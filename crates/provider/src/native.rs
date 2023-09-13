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
    libc::pid_t,
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
    shared::constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_SCRIPTS_DIR},
    DynNamespace, DynNode, ExecutionResult, GenerateFileCommand, GenerateFilesOptions, Provider,
    ProviderCapabilities, ProviderError, ProviderNamespace, ProviderNode, RunCommandOptions,
    RunScriptOptions, SpawnNodeOptions,
};

#[derive(Debug, Clone)]
pub struct NativeProvider<FS: FileSystem + Send + Sync + Clone> {
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    filesystem: FS,
    inner: Arc<RwLock<NativeProviderInner<FS>>>,
}

#[derive(Debug)]
struct NativeProviderInner<FS: FileSystem + Send + Sync + Clone> {
    namespaces: HashMap<String, NativeNamespace<FS>>,
}

#[derive(Debug, Clone)]
struct WeakNativeProvider<FS: FileSystem + Send + Sync + Clone> {
    inner: Weak<RwLock<NativeProviderInner<FS>>>,
}

impl<FS: FileSystem + Send + Sync + Clone> NativeProvider<FS> {
    pub fn new(filesystem: FS) -> Self {
        NativeProvider {
            capabilities: ProviderCapabilities {
                requires_image: false,
            },
            tmp_dir: std::env::temp_dir(),
            filesystem,
            inner: Arc::new(RwLock::new(NativeProviderInner {
                namespaces: Default::default(),
            })),
        }
    }

    pub fn tmp_dir(mut self, tmp_dir: impl Into<PathBuf>) -> Self {
        self.tmp_dir = tmp_dir.into();
        self
    }
}

#[async_trait]
impl<FS: FileSystem + Send + Sync + Clone + 'static> Provider for NativeProvider<FS> {
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError> {
        let id = format!("zombie_{}", Uuid::new_v4());
        let mut inner = self.inner.write().await;

        let base_dir = PathBuf::from(format!("{}/{}", self.tmp_dir.to_string_lossy(), &id));
        self.filesystem.create_dir(&base_dir).await?;

        let namespace = NativeNamespace {
            id: id.clone(),
            base_dir,
            filesystem: self.filesystem.clone(),
            provider: WeakNativeProvider {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(NativeNamespaceInner {
                nodes: Default::default(),
            })),
        };

        inner.namespaces.insert(id, namespace.clone());

        Ok(Arc::new(namespace))
    }
}

#[derive(Debug, Clone)]
pub struct NativeNamespace<FS: FileSystem + Send + Sync + Clone> {
    id: String,
    base_dir: PathBuf,
    inner: Arc<RwLock<NativeNamespaceInner<FS>>>,
    filesystem: FS,
    provider: WeakNativeProvider<FS>,
}

#[derive(Debug)]
struct NativeNamespaceInner<FS: FileSystem + Send + Sync + Clone> {
    nodes: HashMap<String, NativeNode<FS>>,
}

#[derive(Debug, Clone)]
struct WeakNativeNamespace<FS: FileSystem + Send + Sync + Clone> {
    inner: Weak<RwLock<NativeNamespaceInner<FS>>>,
}

#[async_trait]
impl<FS: FileSystem + Send + Sync + Clone + 'static> ProviderNamespace for NativeNamespace<FS> {
    fn id(&self) -> String {
        self.id.clone()
    }

    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        let mut inner = self.inner.write().await;

        if inner.nodes.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name));
        }

        // create node directories and filepaths
        let base_dir_raw = format!("{}/{}", &self.base_dir.to_string_lossy(), &options.name);
        let base_dir = PathBuf::from(&base_dir_raw);
        let log_path = PathBuf::from(format!("{}/{}.log", base_dir_raw, &options.name));
        let config_dir = PathBuf::from(format!("{}/{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}/{}", base_dir_raw, NODE_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}/{}", base_dir_raw, NODE_SCRIPTS_DIR));
        self.filesystem.create_dir(&base_dir).await?;
        self.filesystem.create_dir(&config_dir).await?;
        self.filesystem.create_dir(&data_dir).await?;

        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &options.name,
                &options.command,
                &options.args,
                &options.env,
                &log_path,
                self.filesystem.clone(),
            )?;

        // create node structure holding state
        let node = NativeNode {
            name: options.name.clone(),
            command: options.command,
            args: options.args,
            env: options.env,
            base_dir,
            scripts_dir,
            log_path,
            filesystem: self.filesystem.clone(),
            namespace: WeakNativeNamespace {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(NativeNodeInner {
                process,
                stdout_reading_handle,
                stderr_reading_handle,
                log_writing_handle,
            })),
        };

        // store node inside namespace
        inner.nodes.insert(options.name, node.clone());

        Ok(Arc::new(node))
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        // we spawn a node doing nothing but looping so we can execute our commands
        let temp_node = self
            .spawn_node(SpawnNodeOptions {
                name: format!("temp_{}", Uuid::new_v4()),
                command: "bash".to_string(),
                args: vec!["-c".to_string(), "while :; do sleep 1; done".to_string()],
                env: vec![],
                injected_files: options.injected_files,
            })
            .await?;

        for GenerateFileCommand {
            command,
            args,
            env,
            local_output_path,
        } in options.commands
        {
            match temp_node
                .run_command(RunCommandOptions { command, args, env })
                .await
                .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?
            {
                Ok(contents) => self
                    .filesystem
                    .write(
                        format!(
                            "{}/{}",
                            self.base_dir.to_string_lossy(),
                            local_output_path.to_string_lossy()
                        ),
                        contents,
                    )
                    .await
                    .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?,
                Err((_, msg)) => Err(ProviderError::FileGenerationFailed(anyhow!("{msg}")))?,
            };
        }

        temp_node.destroy().await
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
        if let Some(provider) = self.provider.inner.upgrade() {
            provider.write().await.namespaces.remove(&self.id);
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct NativeNode<FS: FileSystem + Send + Sync + Clone> {
    name: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    base_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    inner: Arc<RwLock<NativeNodeInner>>,
    filesystem: FS,
    namespace: WeakNativeNamespace<FS>,
}

#[derive(Debug)]
struct NativeNodeInner {
    process: Child,
    stdout_reading_handle: JoinHandle<()>,
    stderr_reading_handle: JoinHandle<()>,
    log_writing_handle: JoinHandle<()>,
}

#[async_trait]
impl<FS: FileSystem + Send + Sync + Clone + 'static> ProviderNode for NativeNode<FS> {
    fn name(&self) -> String {
        self.name.clone()
    }

    async fn endpoint(&self) -> Result<(IpAddr, Port), ProviderError> {
        todo!();
    }

    async fn mapped_port(&self, _port: Port) -> Result<Port, ProviderError> {
        todo!()
    }

    async fn logs(&self) -> Result<String, ProviderError> {
        Ok(self.filesystem.read_to_string(&self.log_path).await?)
    }

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError> {
        let logs = self.logs().await?;
        Ok(self.filesystem.write(local_dest, logs.as_bytes()).await?)
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
        let local_script_path = PathBuf::from(&options.local_script_path);

        if !local_script_path
            .try_exists()
            .map_err(|err| ProviderError::InvalidScriptPath(err.into()))?
        {
            return Err(ProviderError::ScriptNotFound(local_script_path));
        }

        // extract file name and build remote file path
        let script_file_name = local_script_path
            .file_name()
            .map(|file_name| file_name.to_string_lossy().to_string())
            .ok_or(ProviderError::InvalidScriptPath(anyhow!(
                "Can't retrieve filename from script with path: {:?}",
                options.local_script_path
            )))?;
        let remote_script_path = format!(
            "{}/{}",
            self.scripts_dir.to_string_lossy(),
            script_file_name
        );

        // copy and set script's execute permission
        self.filesystem
            .copy(local_script_path, &remote_script_path)
            .await?;
        self.filesystem.set_mode(&remote_script_path, 0o744).await?;

        // execute script
        self.run_command(RunCommandOptions {
            command: remote_script_path,
            args: options.args,
            env: options.env,
        })
        .await
    }

    async fn copy_file_from_node(
        &self,
        remote_src: PathBuf,
        local_dest: PathBuf,
    ) -> Result<(), ProviderError> {
        let remote_file_path = format!(
            "{}/{}",
            self.base_dir.to_string_lossy(),
            remote_src.to_string_lossy()
        );
        self.filesystem.copy(remote_file_path, local_dest).await?;

        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let pid = retrieve_pid_from_process(&inner.process, &self.name)?;

        kill(pid, Signal::SIGSTOP)
            .map_err(|_| ProviderError::PauseNodeFailed(self.name.clone()))?;

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let pid = retrieve_pid_from_process(&inner.process, &self.name)?;

        kill(pid, Signal::SIGCONT)
            .map_err(|_| ProviderError::ResumeNodeFaied(self.name.clone()))?;

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
        inner
            .process
            .kill()
            .await
            .map_err(|_| ProviderError::KillNodeFailed(self.name.clone()))?;

        // re-spawn process with tasks for logs
        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &self.name,
                &self.command,
                &self.args,
                &self.env,
                &self.log_path,
                self.filesystem.clone(),
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
        inner
            .process
            .kill()
            .await
            .map_err(|_| ProviderError::KillNodeFailed(self.name.clone()))?;

        if let Some(namespace) = self.namespace.inner.upgrade() {
            namespace.write().await.nodes.remove(&self.name);
        }

        Ok(())
    }
}

fn retrieve_pid_from_process(process: &Child, node_name: &str) -> Result<Pid, ProviderError> {
    Ok(Pid::from_raw(
        process
            .id()
            .ok_or(ProviderError::ProcessIdRetrievalFailed(
                node_name.to_string(),
            ))?
            .try_into()
            .map_err(|_| ProviderError::ProcessIdRetrievalFailed(node_name.to_string()))?,
    ))
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
    log_path: PathBuf,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(250)).await;
            while let Some(Ok(data)) = rx.recv().await {
                // TODO: find a better way instead of ignoring error ?
                let _ = filesystem.append(&log_path, data).await;
            }
        }
    })
}

fn create_process_with_log_tasks(
    name: &str,
    command: &str,
    args: &Vec<String>,
    env: &Vec<(String, String)>,
    log_path: &PathBuf,
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
    use std::{ffi::OsString, str::FromStr};

    use support::fs::{
        in_memory::{InMemoryFile, InMemoryFileSystem},
        local::LocalFileSystem,
    };

    use super::*;

    #[test]
    fn it_should_possible_to_retrieve_capabilities() {
        let fs = InMemoryFileSystem::default();
        let provider = NativeProvider::new(fs);

        let capabilities = provider.capabilities();

        assert_eq!(capabilities.requires_image, false);
    }

    #[tokio::test]
    async fn it_should_be_possible_to_create_a_new_namespace() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let provider = NativeProvider::new(fs.clone());

        let namespace = provider.create_namespace().await.unwrap();

        println!("{:?}", fs.files.read().await);
    }

    #[tokio::test]
    async fn it_works() {
        let fs = LocalFileSystem::default();
        let provider = NativeProvider::new(fs);

        let namespace = provider.create_namespace().await.unwrap();

        namespace
            .generate_files(GenerateFilesOptions {
                commands: vec![GenerateFileCommand {
                    command: "/home/user/.bin/polkadot".to_string(),
                    args: vec![
                        "build-spec".to_string(),
                        "--chain=rococo-local".to_string(),
                        "--disable-default-bootnode".to_string(),
                    ],
                    env: vec![],
                    local_output_path: "rococo-local-plain.json".into(),
                }],
                injected_files: vec![],
            })
            .await
            .unwrap();

        // let node = namespace
        //     .spawn_node(SpawnNodeOptions {
        //         name: "node1".to_string(),
        //         command: "/home/user/.bin/polkadot".to_string(),
        //         args: vec![],
        //         env: vec![],
        //         injected_files: vec![],
        //     })
        //     .await
        //     .unwrap();

        // sleep(Duration::from_secs(10)).await;

        // node.pause().await.unwrap();

        // sleep(Duration::from_secs(10)).await;

        // node.resume().await.unwrap();

        // node.restart(Some(Duration::from_secs(10))).await.unwrap();

        // sleep(Duration::from_secs(10)).await;
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
