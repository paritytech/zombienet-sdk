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
    errors::ProviderError,
    shared::constants::{DEFAULT_TMP_DIR, NODE_CONFIG_DIR, NODE_DATA_DIR},
    DynNamespace, DynNode, ExecutionResult, Provider, ProviderCapabilities, ProviderNamespace,
    ProviderNode, RunCommandOptions, RunScriptOptions, SpawnNodeOptions, SpawnTempOptions,
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
    log_path: String,
    process: Child,
    stdout_reading_handle: JoinHandle<()>,
    stderr_reading_handle: JoinHandle<()>,
    log_writing_handle: JoinHandle<()>,
    filesystem: FS,
    namespace: WeakNativeNamespace<FS>,
}

impl<FS: FileSystem + Send + Sync + Clone> NativeNodeInner<FS> {}

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

    async fn dump_logs(&self, dest: PathBuf) -> Result<(), ProviderError> {
        let logs = self.logs().await?;
        Ok(self
            .inner
            .write()
            .await
            .filesystem
            .write(dest, logs.as_bytes())
            .await?)
    }

    async fn run_command(
        &self,
        _options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        todo!()
    }

    async fn run_script(
        &self,
        _options: RunScriptOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        todo!()
    }

    async fn copy_file_from_node(
        &self,
        _remote_src: PathBuf,
        _local_dest: PathBuf,
    ) -> Result<(), ProviderError> {
        todo!()
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

    async fn restart(&mut self, after: Option<Duration>) -> Result<(), ProviderError> {
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

    // create additonnal long-running tasks for logs
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
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn it_should_works() {
        todo!();
    }
}

// #[derive(Debug, Clone, PartialEq)]
// pub struct NativeProvider<T: FileSystem + Send + Sync> {
//     // Namespace of the client (isolation directory)
//     namespace: String,
//     // TODO: re-iterate, since we are creating the config with the sdk
//     // Path where configuration relies, all the `files` are accessed relative to this.
//     // config_path: String,
//     // Command to use, e.g "bash"
//     command: String,
//     // Temporary directory, root directory for the network
//     tmp_dir: String,
//     remote_dir: String,
//     data_dir: String,
//     process_map: HashMap<String, Process>,
//     filesystem: T,
// }

// impl<T: FileSystem + Send + Sync> NativeProvider<T> {
//     /// Zombienet `native` provider allows to run the nodes as a local process in the local environment
//     /// params:
//     ///   namespace:  Namespace of the client
//     ///   config_path: Path where configuration relies
//     ///   tmp_dir: Temporary directory where files will be placed
//     ///   filesystem: Filesystem to use (std::fs::FileSystem, mock etc.)
//     pub fn new(
//         namespace: impl Into<String>,
//         // config_path: impl Into<String>,
//         tmp_dir: impl Into<String>,
//         filesystem: T,
//     ) -> Self {
//         let tmp_dir = tmp_dir.into();
//         let process_map: HashMap<String, Process> = HashMap::new();

//         Self {
//             namespace: namespace.into(),
//             // config_path: config_path.into(),
//             remote_dir: format!("{}{}", &tmp_dir, DEFAULT_REMOTE_DIR),
//             data_dir: format!("{}{}", &tmp_dir, DEFAULT_DATA_DIR),
//             command: "bash".into(),
//             tmp_dir,
//             process_map,
//             filesystem,
//         }
//     }

//     fn get_process_by_node_name(&self, node_name: &str) -> Result<&Process, ProviderError> {
//         self.process_map
//             .get(node_name)
//             .ok_or(ProviderError::MissingNodeInfo(
//                 node_name.to_owned(),
//                 "process".into(),
//             ))
//     }
// }

// pub struct Node {}

// #[async_trait]
// impl<T> Provider for NativeProvider<T>
// where
//     T: FileSystem + Send + Sync,
// {
//     type Node = Node;

//     fn require_image() -> bool {
//         false
//     }

//     async fn create_namespace(&mut self) -> Result<(), ProviderError> {
//         // Native provider don't have the `namespace` isolation.
//         // but we create the `remoteDir` to place files
//         self.filesystem
//             .create_dir(&self.remote_dir)
//             .await
//             .map_err(|e| ProviderError::FSError(Box::new(e)))?;
//         Ok(())
//     }

//     async fn destroy_namespace(&self) -> Result<(), ProviderError> {
//         // get pids to kill all related process
//         let pids: Vec<String> = self
//             .process_map
//             .iter()
//             .filter(|(_, process)| process.pid != 0)
//             .map(|(_, process)| process.pid.to_string())
//             .collect();

//         // TODO: use a crate (or even std) to get this info instead of relying on bash
//         let result = self
//             .run_command(
//                 [format!(
//                     "ps ax| awk '{{print $1}}'| grep -E '{}'",
//                     pids.join("|")
//                 )]
//                 .to_vec(),
//                 NativeRunCommandOptions {
//                     is_failure_allowed: true,
//                 },
//             )
//             .await
//             .unwrap();

//         if result.exit_code.code().unwrap() == 0 {
//             let pids_to_kill: Vec<String> = result
//                 .std_out
//                 .split(|c| c == '\n')
//                 .map(|s| s.into())
//                 .collect();

//             let _ = self
//                 .run_command(
//                     [format!("kill -9 {}", pids_to_kill.join(" "))].to_vec(),
//                     NativeRunCommandOptions {
//                         is_failure_allowed: true,
//                     },
//                 )
//                 .await?;
//         }
//         Ok(())
//     }

//     async fn static_setup(&mut self) -> Result<(), ProviderError> {
//         Ok(())
//     }

//     async fn spawn_node(
//         &self,
//         _node: Node,
//         _files_inject: Vec<FileMap>,
//         _keystore: &str,
//         _db_snapshot: &str,
//     ) -> Result<(), ProviderError> {
//         // TODO: We should implement the logic to go from the `Node` (nodeSpec)
//         // to the running node, since we will no expose anymore the underline `Def`.
//         // We can follow the logic of the spawn_from_def later.

//         Ok(())
//     }

//     async fn spawn_temp(
//         &self,
//         _node: Node,
//         _files_inject: Vec<FileMap>,
//         _files_get: Vec<FileMap>,
//     ) -> Result<(), ProviderError> {
//         // TODO: We should implement the logic to go from the `Node` (nodeSpec)
//         // to the running node, since we will no expose anymore the underline `Def`.
//         // We can follow the logic of the spawn_from_def later.

//         Ok(())
//     }

//     async fn copy_file_from_node(
//         &mut self,
//         pod_file_path: PathBuf,
//         local_file_path: PathBuf,
//     ) -> Result<(), ProviderError> {
//         // log::debug!("cp {} {}", pod_file_path.to_string_lossy(), local_file_path.to_string_lossy());

//         self.filesystem
//             .copy(&pod_file_path, &local_file_path)
//             .await
//             .map_err(|e| ProviderError::FSError(Box::new(e)))?;
//         Ok(())
//     }

//     async fn run_command(
//         &self,
//         mut args: Vec<String>,
//         opts: NativeRunCommandOptions,
//     ) -> Result<RunCommandResponse, ProviderError> {
//         if let Some(arg) = args.get(0) {
//             if arg == "bash" {
//                 args.remove(0);
//             }
//         }

//         // -c is already used in the process::Command to execute the command thus
//         // needs to be removed in case provided
//         if let Some(arg) = args.get(0) {
//             if arg == "-c" {
//                 args.remove(0);
//             }
//         }

//         let result = Command::new(&self.command)
//             .arg("-c")
//             .arg(args.join(" "))
//             .output()
//             .await?;

//         if !result.status.success() && !opts.is_failure_allowed {
//             return Err(ProviderError::RunCommandError(args.join(" ")));
//         } else {
//             // cmd success or we allow to fail
//             // in either case we return Ok
//             Ok(RunCommandResponse {
//                 exit_code: result.status,
//                 std_out: String::from_utf8_lossy(&result.stdout).into(),
//                 std_err: if result.stderr.is_empty() {
//                     None
//                 } else {
//                     Some(String::from_utf8_lossy(&result.stderr).into())
//                 },
//             })
//         }
//     }

//     // TODO: Add test
//     async fn run_script(
//         &mut self,
//         identifier: String,
//         script_path: String,
//         args: Vec<String>,
//     ) -> Result<RunCommandResponse, ProviderError> {
//         let script_filename = Path::new(&script_path)
//             .file_name()
//             .ok_or(ProviderError::InvalidScriptPath(script_path.clone()))?
//             .to_str()
//             .ok_or(ProviderError::InvalidScriptPath(script_path.clone()))?;
//         let script_path_in_pod = format!("{}/{}/{}", self.tmp_dir, identifier, script_filename);

//         // upload the script
//         self.filesystem
//             .copy(&script_path, &script_path_in_pod)
//             .await
//             .map_err(|e| ProviderError::FSError(Box::new(e)))?;

//         // set as executable
//         self.run_command(
//             vec![
//                 "chmod".to_owned(),
//                 "+x".to_owned(),
//                 script_path_in_pod.clone(),
//             ],
//             NativeRunCommandOptions::default(),
//         )
//         .await?;

//         let command = format!(
//             "cd {}/{} && {} {}",
//             self.tmp_dir,
//             identifier,
//             script_path_in_pod,
//             args.join(" ")
//         );
//         let result = self
//             .run_command(vec![command], NativeRunCommandOptions::default())
//             .await?;

//         Ok(RunCommandResponse {
//             exit_code: result.exit_code,
//             std_out: result.std_out,
//             std_err: result.std_err,
//         })
//     }

//     // TODO: Add test
//     async fn get_node_logs(&mut self, name: &str) -> Result<String, ProviderError> {
//         // For now in native let's just return all the logs
//         let result = self
//             .filesystem
//             .read_file(&format!("{}/{}.log", self.tmp_dir, name))
//             .await
//             .map_err(|e| ProviderError::FSError(Box::new(e)))?;
//         return Ok(result);
//     }

//     async fn dump_logs(&mut self, path: String, pod_name: String) -> Result<(), ProviderError> {
//         let dst_file_name: String = format!("{}/logs/{}.log", path, pod_name);
//         let _ = self
//             .filesystem
//             .copy(
//                 &format!("{}/{}.log", self.tmp_dir, pod_name),
//                 &dst_file_name,
//             )
//             .await;
//         Ok(())
//     }

//     async fn get_logs_command(&self, name: &str) -> Result<String, ProviderError> {
//         Ok(format!("tail -f {}/{}.log", self.tmp_dir, name))
//     }

//     // TODO: Add test
//     async fn restart(
//         &mut self,
//         node_name: &str,
//         after_secs: Option<u16>,
//     ) -> Result<bool, ProviderError> {
//         let process = self.get_process_by_node_name(node_name)?;

//         let _resp = self
//             .run_command(
//                 vec![format!("kill -9 {:?}", process.pid)],
//                 NativeRunCommandOptions {
//                     is_failure_allowed: true,
//                 },
//             )
//             .await?;

//         // log::debug!("{:?}", &resp);

//         if let Some(secs) = after_secs {
//             sleep(Duration::from_secs(secs.into())).await;
//         }

//         let process: &mut Process =
//             self.process_map
//                 .get_mut(node_name)
//                 .ok_or(ProviderError::MissingNodeInfo(
//                     node_name.to_owned(),
//                     "process".into(),
//                 ))?;

//         let mapped_env: HashMap<&str, &str> = process
//             .env
//             .iter()
//             .map(|env_var| (env_var.name.as_str(), env_var.value.as_str()))
//             .collect();

//         let child_process: Child = Command::new(self.command.clone())
//             .arg("-c")
//             .arg(process.command.clone())
//             .envs(&mapped_env)
//             .spawn()
//             .map_err(|e| ProviderError::ErrorSpawningNode(e.to_string()))?;

//         process.pid = child_process.id().ok_or(ProviderError::ErrorSpawningNode(
//             "Failed to get pid".to_string(),
//         ))?;

//         Ok(true)
//     }

//     async fn get_node_info(&self, node_name: &str) -> Result<(IpAddr, Port), ProviderError> {
//         let host_port = self.get_port_mapping(P2P_PORT, node_name).await?;
//         Ok((LOCALHOST, host_port))
//     }

//     async fn get_node_ip(&self, _node_name: &str) -> Result<IpAddr, ProviderError> {
//         Ok(LOCALHOST)
//     }

//     async fn get_port_mapping(&self, port: Port, node_name: &str) -> Result<Port, ProviderError> {
//         match self.process_map.get(node_name) {
//             Some(process) => match process.port_mapping.get(&port) {
//                 Some(port) => Ok(*port),
//                 None => Err(ProviderError::MissingNodeInfo(
//                     node_name.to_owned(),
//                     "port".into(),
//                 )),
//             },
//             None => Err(ProviderError::MissingNodeInfo(
//                 node_name.to_owned(),
//                 "process".into(),
//             )),
//         }
//     }
// }

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
