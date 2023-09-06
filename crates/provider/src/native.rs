use std::{
    self,
    collections::HashMap,
    fmt::Debug,
    io::Error,
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::types::Port;
use nix::{
    sys::signal::{kill, Signal},
    unistd::Pid,
};
use support::fs::FileSystem;
use tokio::{
    io::{AsyncRead, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{
        mpsc::{self, Receiver, Sender},
        RwLock,
    },
    task::JoinHandle,
    time::{sleep, Duration},
};
use uuid::Uuid;

use crate::{
    shared::constants::{DEFAULT_TMP_DIR, NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_SCRIPTS_DIR},
    DynNamespace, DynNode, ExecutionResult, Provider, ProviderCapabilities, ProviderError,
    ProviderNamespace, ProviderNode, RunCommandOptions, RunScriptOptions, SpawnNodeOptions,
    SpawnTempOptions,
};

pub struct NativeProviderOptions<FS>
where
    FS: FileSystem + Send + Sync,
{
    filesystem: FS,
    tmp_dir: Option<String>,
}

#[derive(Debug)]
struct NativeProviderInner<FS: FileSystem + Send + Sync + Clone> {
    capabilities: ProviderCapabilities,
    tmp_dir: String,
    namespaces: HashMap<String, NativeNamespace<FS>>,
    filesystem: FS,
}

#[derive(Debug, Clone)]
pub struct NativeProvider<FS: FileSystem + Send + Sync + Clone> {
    inner: Arc<RwLock<NativeProviderInner<FS>>>,
}

#[derive(Debug, Clone)]
struct WeakNativeProvider<FS: FileSystem + Send + Sync + Clone> {
    inner: Weak<RwLock<NativeProviderInner<FS>>>,
}

impl<FS: FileSystem + Send + Sync + Clone> NativeProvider<FS> {
    pub fn new(options: NativeProviderOptions<FS>) -> Self {
        NativeProvider {
            inner: Arc::new(RwLock::new(NativeProviderInner {
                capabilities: ProviderCapabilities {
                    requires_image: false,
                },
                tmp_dir: options.tmp_dir.unwrap_or(DEFAULT_TMP_DIR.to_string()),
                namespaces: Default::default(),
                filesystem: options.filesystem,
            })),
        }
    }
}

#[async_trait]
impl<FS: FileSystem + Send + Sync + Clone + 'static> Provider for NativeProvider<FS> {
    async fn capabilities(&self) -> ProviderCapabilities {
        self.inner.read().await.capabilities.clone()
    }

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError> {
        let id = format!("zombie_{}", Uuid::new_v4());
        let mut inner = self.inner.write().await;

        let base_dir = format!("{}/{}", inner.tmp_dir, &id);
        inner.filesystem.create_dir(&base_dir).await.unwrap();

        let namespace = NativeNamespace {
            inner: Arc::new(RwLock::new(NativeNamespaceInner {
                id: id.clone(),
                base_dir,
                nodes: Default::default(),
                filesystem: inner.filesystem.clone(),
                provider: WeakNativeProvider {
                    inner: Arc::downgrade(&self.inner),
                },
            })),
        };

        inner.namespaces.insert(id, namespace.clone());

        Ok(Arc::new(namespace))
    }
}

#[derive(Debug)]
struct NativeNamespaceInner<FS: FileSystem + Send + Sync + Clone> {
    id: String,
    base_dir: String,
    nodes: HashMap<String, NativeNode<FS>>,
    filesystem: FS,
    provider: WeakNativeProvider<FS>,
}

#[derive(Debug, Clone)]
pub struct NativeNamespace<FS: FileSystem + Send + Sync + Clone> {
    inner: Arc<RwLock<NativeNamespaceInner<FS>>>,
}

#[derive(Debug, Clone)]
struct WeakNativeNamespace<FS: FileSystem + Send + Sync + Clone> {
    inner: Weak<RwLock<NativeNamespaceInner<FS>>>,
}

#[async_trait]
impl<FS: FileSystem + Send + Sync + Clone + 'static> ProviderNamespace for NativeNamespace<FS> {
    async fn id(&self) -> String {
        self.inner.read().await.id.clone()
    }

    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        let mut inner = self.inner.write().await;

        // create node directories and filepaths
        let base_dir = format!("{}/{}", &inner.base_dir, &options.name);
        let log_path = format!("{}/{}.log", &base_dir, &options.name);
        let config_dir = format!("{}{}", &base_dir, NODE_CONFIG_DIR);
        let data_dir = format!("{}{}", &base_dir, NODE_DATA_DIR);
        let scripts_dir = format!("{}{}", &base_dir, NODE_SCRIPTS_DIR);
        inner.filesystem.create_dir(&base_dir).await.unwrap();
        inner.filesystem.create_dir(&config_dir).await.unwrap();
        inner.filesystem.create_dir(&data_dir).await.unwrap();

        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &options.name,
                &options.command,
                &options.args,
                &options.env,
                &log_path,
                inner.filesystem.clone(),
            )?;

        // create node structure holding state
        let node = NativeNode {
            inner: Arc::new(RwLock::new(NativeNodeInner {
                name: options.name.clone(),
                command: options.command,
                args: options.args,
                env: options.env,
                base_dir,
                scripts_dir,
                log_path,
                process,
                stdout_reading_handle,
                stderr_reading_handle,
                log_writing_handle,
                filesystem: inner.filesystem.clone(),
                namespace: WeakNativeNamespace {
                    inner: Arc::downgrade(&self.inner),
                },
            })),
        };

        // store node inside namespace
        inner.nodes.insert(options.name, node.clone());

        Ok(Arc::new(node))
    }

    async fn spawn_temp(&self, _options: SpawnTempOptions) -> Result<(), ProviderError> {
        todo!()
    }

    async fn static_setup(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        // we need to clone nodes (behind an Arc, so cheaply) to avoid deadlock between the inner.write lock and the node.destroy
        // method acquiring a lock the namespace to remove the node from the nodes hashmap.
        let nodes = self
            .inner
            .write()
            .await
            .nodes
            .iter()
            .map(|(_, node)| node.clone())
            .collect::<Vec<NativeNode<FS>>>();

        for node in nodes.iter() {
            node.destroy().await?;
        }

        // remove namespace from provider
        let inner = self.inner.write().await;
        if let Some(provider) = inner.provider.inner.upgrade() {
            provider.write().await.namespaces.remove(&inner.id);
        }

        Ok(())
    }
}

#[derive(Debug)]
struct NativeNodeInner<FS: FileSystem + Send + Sync + Clone> {
    name: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    base_dir: String,
    scripts_dir: String,
    log_path: String,
    process: Child,
    stdout_reading_handle: JoinHandle<()>,
    stderr_reading_handle: JoinHandle<()>,
    log_writing_handle: JoinHandle<()>,
    filesystem: FS,
    namespace: WeakNativeNamespace<FS>,
}

#[derive(Debug, Clone)]
struct NativeNode<FS: FileSystem + Send + Sync + Clone> {
    inner: Arc<RwLock<NativeNodeInner<FS>>>,
}

#[async_trait]
impl<FS: FileSystem + Send + Sync + Clone + 'static> ProviderNode for NativeNode<FS> {
    async fn name(&self) -> String {
        self.inner.read().await.name.clone()
    }

    async fn endpoint(&self) -> Result<(IpAddr, Port), ProviderError> {
        todo!();
    }

    async fn mapped_port(&self, _port: Port) -> Result<Port, ProviderError> {
        todo!()
    }

    async fn logs(&self) -> Result<String, ProviderError> {
        let inner = self.inner.read().await;
        Ok(inner.filesystem.read_to_string(&inner.log_path).await?)
    }

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError> {
        let logs = self.logs().await?;
        Ok(self
            .inner
            .write()
            .await
            .filesystem
            .write(local_dest, logs.as_bytes())
            .await?)
    }

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let result = Command::new(options.command)
            .args(options.args)
            .output()
            .await
            .map_err(|err| ProviderError::RunCommandError(err.into()))?;

        if result.status.success() {
            Ok(Ok(String::from_utf8_lossy(&result.stdout).to_string()))
        } else {
            Ok(Err((
                result.status,
                String::from_utf8_lossy(&result.stderr).to_string(),
            )))
        }
    }

    async fn run_script(
        &self,
        options: RunScriptOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let inner = self.inner.read().await;
        let local_script_path = PathBuf::from(&options.local_script_path);

        if !local_script_path.try_exists().unwrap() {
            return Err(ProviderError::RunCommandError(anyhow!("Test")));
        }

        // extract file name and build remote file path
        let script_file_name = local_script_path
            .file_name()
            .map(|file_name| file_name.to_string_lossy().to_string())
            .ok_or(ProviderError::InvalidScriptPath(options.local_script_path))?;
        let remote_script_path = format!("{}/{}", inner.scripts_dir, script_file_name);

        // copy and set script's execute permission
        inner
            .filesystem
            .copy(local_script_path, &remote_script_path)
            .await?;
        inner
            .filesystem
            .set_mode(&remote_script_path, 0o744)
            .await?;

        // execute script
        self.run_command(RunCommandOptions::new(remote_script_path).args(options.args))
            .await
    }

    async fn copy_file_from_node(
        &self,
        remote_src: PathBuf,
        local_dest: PathBuf,
    ) -> Result<(), ProviderError> {
        let inner = self.inner.read().await;

        let remote_file_path = format!("{}{}", inner.base_dir, remote_src.to_str().unwrap());
        inner.filesystem.copy(remote_file_path, local_dest).await?;

        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let raw_pid = inner.process.id().unwrap();
        let pid = Pid::from_raw(raw_pid.try_into().unwrap());

        kill(pid, Signal::SIGSTOP).unwrap();

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let raw_pid = inner.process.id().unwrap();
        let pid = Pid::from_raw(raw_pid.try_into().unwrap());

        kill(pid, Signal::SIGCONT).unwrap();

        Ok(())
    }

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError> {
        if let Some(duration) = after {
            sleep(duration).await;
        }

        let mut inner = self.inner.write().await;

        // abort all task handlers and kill process
        inner.log_writing_handle.abort();
        inner.stdout_reading_handle.abort();
        inner.stderr_reading_handle.abort();
        inner.process.kill().await.unwrap();

        // re-spawn process with tasks for logs
        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &inner.name,
                &inner.command,
                &inner.args,
                &inner.env,
                &inner.log_path,
                inner.filesystem.clone(),
            )?;

        // update node process and handlers
        inner.process = process;
        inner.stderr_reading_handle = stdout_reading_handle;
        inner.stderr_reading_handle = stderr_reading_handle;
        inner.log_writing_handle = log_writing_handle;

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.write().await;

        inner.log_writing_handle.abort();
        inner.stdout_reading_handle.abort();
        inner.stderr_reading_handle.abort();
        inner.process.kill().await.unwrap();

        if let Some(namespace) = inner.namespace.inner.upgrade() {
            namespace.write().await.nodes.remove(&inner.name);
        }

        Ok(())
    }
}

fn create_stream_polling_task(
    stream: impl AsyncRead + Unpin + Send + 'static,
    tx: Sender<Result<Vec<u8>, Error>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut buffer = vec![0u8; 1024];

        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => {
                    let _ = tx.send(Ok(Vec::new())).await;
                    break;
                },
                Ok(n) => {
                    let _ = tx.send(Ok(buffer[..n].to_vec())).await;
                },
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                },
            }
        }
    })
}

fn create_log_writing_task(
    mut rx: Receiver<Result<Vec<u8>, Error>>,
    filesystem: impl FileSystem + Send + Sync + 'static,
    log_path: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(250)).await;
            while let Some(Ok(data)) = rx.recv().await {
                filesystem.append(&log_path, data).await.unwrap();
            }
        }
    })
}

fn create_process_with_log_tasks(
    name: &str,
    command: &str,
    args: &[String],
    env: &[(String, String)],
    log_path: &str,
    filesystem: impl FileSystem + Send + Sync + 'static,
) -> Result<(Child, JoinHandle<()>, JoinHandle<()>, JoinHandle<()>), ProviderError> {
    // create process
    let mut process = Command::new(command)
        .args(args)
        .envs(env.to_owned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| ProviderError::NodeSpawningFailed(name.to_string(), err.into()))?;
    let stdout = process.stdout.take().expect("infaillible, stdout is piped");
    let stderr = process.stderr.take().expect("Infaillible, stderr is piped");

    // create additionnal long-running tasks for logs
    let (stdout_tx, rx) = mpsc::channel(10);
    let stderr_tx = stdout_tx.clone();
    let stdout_reading_handle = create_stream_polling_task(stdout, stdout_tx);
    let stderr_reading_handle = create_stream_polling_task(stderr, stderr_tx);
    let log_writing_handle = create_log_writing_task(rx, filesystem, log_path.to_owned());

    Ok((
        process,
        stdout_reading_handle,
        stderr_reading_handle,
        log_writing_handle,
    ))
}

#[cfg(test)]
mod tests {
    use std::os::unix::prelude::PermissionsExt;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn it_should_works() {
        let file = std::fs::File::create(format!(
            "{}/{}",
            std::env::temp_dir().to_string_lossy(),
            Uuid::new_v4()
        ))
        .unwrap();

        let metadata = file.metadata().unwrap();

        let mut permissions = metadata.permissions();
        permissions.set_mode(0o744);

        tokio::fs::set_permissions("/tmp/myscript.sh", permissions)
            .await
            .unwrap();

        // let result = Command::new("/tmp/myscript.sh").output().await.unwrap();

        // println!("{:?}", result);
    }
}

// #[cfg(test)]
// mod tests {
//     use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

//     use support::fs::mock::{MockError, MockFilesystem, Operation};

//     use super::*;

//     #[test]
//     fn new_native_provider() {
//         let native_provider: NativeProvider<MockFilesystem> =
//             NativeProvider::new("something", "/tmp", MockFilesystem::new());

//         assert_eq!(native_provider.namespace, "something");
//         assert_eq!(native_provider.tmp_dir, "/tmp");
//         assert_eq!(native_provider.command, "bash");
//         assert_eq!(native_provider.remote_dir, "/tmp/cfg");
//         assert_eq!(native_provider.data_dir, "/tmp/data");
//     }

//     #[tokio::test]
//     async fn test_fielsystem_usage() {
//         let mut native_provider: NativeProvider<MockFilesystem> =
//             NativeProvider::new("something", "/tmp", MockFilesystem::new());

//         native_provider.create_namespace().await.unwrap();

//         assert!(native_provider.filesystem.operations.len() == 1);

//         assert_eq!(
//             native_provider.filesystem.operations[0],
//             Operation::CreateDir {
//                 path: "/tmp/cfg".into(),
//             }
//         );
//     }

//     #[tokio::test]
//     #[should_panic(expected = "FSError(OpError(\"create\"))")]
//     async fn test_fielsystem_usage_fails() {
//         let mut native_provider: NativeProvider<MockFilesystem> = NativeProvider::new(
//             "something",
//             "/tmp",
//             MockFilesystem::with_create_dir_error(MockError::OpError("create".into())),
//         );

//         native_provider.create_namespace().await.unwrap();
//     }

//     #[tokio::test]
//     async fn test_get_node_ip() {
//         let native_provider: NativeProvider<MockFilesystem> =
//             NativeProvider::new("something", "/tmp", MockFilesystem::new());

//         assert_eq!(
//             native_provider.get_node_ip("some").await.unwrap(),
//             LOCALHOST
//         );
//     }

//     #[tokio::test]
//     async fn test_run_command_when_bash_is_removed() {
//         let native_provider: NativeProvider<MockFilesystem> =
//             NativeProvider::new("something", "/tmp", MockFilesystem::new());

//         let result: RunCommandResponse = native_provider
//             .run_command(
//                 vec!["bash".into(), "ls".into()],
//                 NativeRunCommandOptions::default(),
//             )
//             .await
//             .unwrap();

//         assert_eq!(
//             result,
//             RunCommandResponse {
//                 exit_code: ExitStatus::from_raw(0),
//                 std_out: "Cargo.toml\nsrc\n".into(),
//                 std_err: None,
//             }
//         );
//     }

//     #[tokio::test]
//     async fn test_run_command_when_dash_c_is_provided() {
//         let native_provider = NativeProvider::new("something", "/tmp", MockFilesystem::new());

//         let result = native_provider.run_command(
//             vec!["-c".into(), "ls".into()],
//             NativeRunCommandOptions::default(),
//         );

//         let a = result.await;
//         assert!(a.is_ok());
//     }

//     #[tokio::test]
//     async fn test_run_command_when_error_return_error() {
//         let native_provider = NativeProvider::new("something", "/tmp", MockFilesystem::new());

//         let mut some = native_provider.run_command(
//             vec!["ls".into(), "ls".into()],
//             NativeRunCommandOptions::default(),
//         );

//         assert!(some.await.is_err());

//         some = native_provider.run_command(
//             vec!["ls".into(), "ls".into()],
//             NativeRunCommandOptions {
//                 is_failure_allowed: true,
//             },
//         );

//         assert!(some.await.is_ok());
//     }
// }
