use std::{
    self,
    collections::HashMap,
    io::Error,
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::types::Port;
use futures::{future::try_join_all, try_join};
use nix::{sys::signal::Signal, unistd::Pid};
use support::{
    fs::FileSystem,
    process::{Command, DynProcess, ProcessManager},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, BufReader},
    sync::{
        mpsc::{self, Receiver, Sender},
        RwLock,
    },
    task::JoinHandle,
    time::{sleep, Duration},
};
use uuid::Uuid;

use crate::{
    constants::LOCALHOST,
    shared::constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_SCRIPTS_DIR},
    DynNamespace, DynNode, ExecutionResult, GenerateFileCommand, GenerateFilesOptions, Provider,
    ProviderCapabilities, ProviderError, ProviderNamespace, ProviderNode, RunCommandOptions,
    RunScriptOptions, SpawnNodeOptions,
};

#[derive(Clone)]
pub struct NativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    filesystem: FS,
    process_manager: PM,
    inner: Arc<RwLock<NativeProviderInner<FS, PM>>>,
}

struct NativeProviderInner<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    namespaces: HashMap<String, NativeNamespace<FS, PM>>,
}

#[derive(Clone)]
struct WeakNativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    inner: Weak<RwLock<NativeProviderInner<FS, PM>>>,
}

impl<FS, PM> NativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub fn new(filesystem: FS, process_manager: PM) -> Self {
        NativeProvider {
            capabilities: ProviderCapabilities::new(),
            // NOTE: temp_dir in linux return `/tmp` but on mac something like
            //  `/var/folders/rz/1cyx7hfj31qgb98d8_cg7jwh0000gn/T/`, having
            // one `trailing slash` and the other no can cause issues if
            // you try to build a fullpath by concatenate. Use Pathbuf to prevent the issue.
            tmp_dir: std::env::temp_dir(),
            filesystem,
            process_manager,
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
impl<FS, PM> Provider for NativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    PM: ProcessManager + Send + Sync + Clone + 'static,
{
    fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    async fn namespaces(&self) -> HashMap<String, DynNamespace> {
        self.inner
            .read()
            .await
            .namespaces
            .clone()
            .into_iter()
            .map(|(id, namespace)| (id, Arc::new(namespace) as DynNamespace))
            .collect()
    }

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError> {
        let id = format!("zombie_{}", Uuid::new_v4());
        let mut inner = self.inner.write().await;

        let base_dir = PathBuf::from_iter([&self.tmp_dir, &PathBuf::from(&id)]);
        self.filesystem.create_dir(&base_dir).await?;

        let namespace = NativeNamespace {
            id: id.clone(),
            base_dir,
            filesystem: self.filesystem.clone(),
            process_manager: self.process_manager.clone(),
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

#[derive(Clone)]
pub struct NativeNamespace<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    id: String,
    base_dir: PathBuf,
    inner: Arc<RwLock<NativeNamespaceInner<FS, PM>>>,
    filesystem: FS,
    process_manager: PM,
    provider: WeakNativeProvider<FS, PM>,
}

struct NativeNamespaceInner<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    nodes: HashMap<String, NativeNode<FS, PM>>,
}

#[derive(Clone)]
struct WeakNativeNamespace<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    inner: Weak<RwLock<NativeNamespaceInner<FS, PM>>>,
}

#[async_trait]
impl<FS, PM> ProviderNamespace for NativeNamespace<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    PM: ProcessManager + Send + Sync + Clone + 'static,
{
    fn id(&self) -> &str {
        &self.id
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    async fn nodes(&self) -> HashMap<String, DynNode> {
        self.inner
            .read()
            .await
            .nodes
            .clone()
            .into_iter()
            .map(|(id, node)| (id, Arc::new(node) as DynNode))
            .collect()
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
        let config_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_SCRIPTS_DIR));
        // NOTE: in native this base path already exist
        self.filesystem.create_dir_all(&base_dir).await?;
        try_join!(
            self.filesystem.create_dir(&config_dir),
            self.filesystem.create_dir(&data_dir),
            self.filesystem.create_dir(&scripts_dir),
        )?;

        // Created needed paths
        let ops_fut: Vec<_> = options
            .created_paths
            .iter()
            .map(|created_path| {
                self.filesystem.create_dir_all(format!(
                    "{}{}",
                    &base_dir.to_string_lossy(),
                    created_path.to_string_lossy()
                ))
            })
            .collect();
        try_join_all(ops_fut).await?;

        // copy injected files
        let ops_fut: Vec<_> = options
            .injected_files
            .iter()
            .map(|file| {
                self.filesystem.copy(
                    &file.local_path,
                    format!("{}{}", base_dir_raw, file.remote_path.to_string_lossy()),
                )
            })
            .collect();
        try_join_all(ops_fut).await?;

        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &options.name,
                &options.program,
                &options.args,
                &options.env,
                &log_path,
                self.filesystem.clone(),
                self.process_manager.clone(),
            )
            .await?;

        // create node structure holding state
        let node = NativeNode {
            name: options.name.clone(),
            command: options.program,
            args: options.args,
            env: options.env,
            base_dir,
            config_dir,
            data_dir,
            scripts_dir,
            log_path,
            filesystem: self.filesystem.clone(),
            process_manager: self.process_manager.clone(),
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
                program: "bash".to_string(),
                args: vec!["-c".to_string(), "while :; do sleep 1; done".to_string()],
                env: vec![],
                injected_files: options.injected_files,
                created_paths: vec![],
            })
            .await?;

        for GenerateFileCommand {
            program,
            args,
            env,
            local_output_path,
        } in options.commands
        {
            // TODO: move to logger
            // println!("{:#?}, {:#?}", command, args);
            // println!("{:#?}", self.base_dir.to_string_lossy());
            // println!("{:#?}", local_output_path.as_os_str());
            let local_output_full_path = format!(
                "{}{}{}",
                self.base_dir.to_string_lossy(),
                if local_output_path.starts_with("/") {
                    ""
                } else {
                    "/"
                },
                local_output_path.to_string_lossy()
            );

            match temp_node
                .run_command(RunCommandOptions { program, args, env })
                .await
                .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?
            {
                Ok(contents) => self
                    .filesystem
                    .write(local_output_full_path, contents)
                    .await
                    .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?,
                Err((_, msg)) => Err(ProviderError::FileGenerationFailed(anyhow!("{msg}")))?,
            };
        }

        temp_node.destroy().await
    }

    async fn static_setup(&self) -> Result<(), ProviderError> {
        // no static setup exists for native provider
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        // we need to clone nodes (behind an Arc, so cheaply) to avoid deadlock between the inner.write lock and the node.destroy
        // method acquiring a lock the namespace to remove the node from the nodes hashmap.
        let nodes: Vec<NativeNode<FS, PM>> =
            self.inner.write().await.nodes.values().cloned().collect();
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

#[derive(Clone)]
struct NativeNode<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    name: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    inner: Arc<RwLock<NativeNodeInner>>,
    filesystem: FS,
    process_manager: PM,
    namespace: WeakNativeNamespace<FS, PM>,
}

struct NativeNodeInner {
    process: DynProcess,
    stdout_reading_handle: JoinHandle<()>,
    stderr_reading_handle: JoinHandle<()>,
    log_writing_handle: JoinHandle<()>,
}

#[async_trait]
impl<FS, PM> ProviderNode for NativeNode<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    PM: ProcessManager + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn command(&self) -> &str {
        &self.command
    }

    fn args(&self) -> Vec<&String> {
        self.args.iter().collect::<Vec<&String>>()
    }

    async fn ip(&self) -> Result<IpAddr, ProviderError> {
        Ok(LOCALHOST)
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    fn scripts_dir(&self) -> &PathBuf {
        &self.scripts_dir
    }

    fn log_path(&self) -> &PathBuf {
        &self.log_path
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
        Ok(self.filesystem.copy(&self.log_path, local_dest).await?)
    }

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let result = self
            .process_manager
            .output(
                Command::new(options.program.clone())
                    .args(options.args)
                    .envs(options.env),
            )
            .await
            .map_err(|err| ProviderError::RunCommandError(options.program, err.into()))?;

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

        if !self.filesystem.exists(&local_script_path).await {
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
            program: remote_script_path,
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
            "{}{}",
            self.base_dir.to_string_lossy(),
            remote_src.to_string_lossy()
        );
        self.filesystem.copy(remote_file_path, local_dest).await?;

        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let pid = retrieve_pid_from_process(&inner.process, &self.name).await?;

        self.process_manager
            .kill(pid, Signal::SIGSTOP)
            .await
            .map_err(|_| ProviderError::PauseNodeFailed(self.name.clone()))?;

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let pid = retrieve_pid_from_process(&inner.process, &self.name).await?;

        self.process_manager
            .kill(pid, Signal::SIGCONT)
            .await
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
                self.process_manager.clone(),
            )
            .await?;

        // update node process and handlers
        inner.process = process;
        inner.stderr_reading_handle = stdout_reading_handle;
        inner.stderr_reading_handle = stderr_reading_handle;
        inner.log_writing_handle = log_writing_handle;

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;

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

async fn retrieve_pid_from_process(
    process: &DynProcess,
    node_name: &str,
) -> Result<Pid, ProviderError> {
    Ok(Pid::from_raw(
        process
            .id()
            .await
            .ok_or(ProviderError::ProcessIdRetrievalFailed(
                node_name.to_string(),
            ))?
            .try_into()
            .map_err(|_| ProviderError::ProcessIdRetrievalFailed(node_name.to_string()))?,
    ))
}

fn create_stream_polling_task<S>(stream: S, tx: Sender<Result<Vec<u8>, Error>>) -> JoinHandle<()>
where
    S: AsyncRead + Unpin + Send + 'static,
{
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

fn create_log_writing_task<FS>(
    mut rx: Receiver<Result<Vec<u8>, Error>>,
    filesystem: FS,
    log_path: PathBuf,
) -> JoinHandle<()>
where
    FS: FileSystem + Send + Sync + 'static,
{
    tokio::spawn(async move {
        loop {
            while let Some(Ok(data)) = rx.recv().await {
                // TODO: find a better way instead of ignoring error ?
                let _ = filesystem.append(&log_path, data).await;
            }
            sleep(Duration::from_millis(250)).await;
        }
    })
}

type CreateProcessOutput = (DynProcess, JoinHandle<()>, JoinHandle<()>, JoinHandle<()>);

async fn create_process_with_log_tasks<FS, PM>(
    name: &str,
    program: &str,
    args: &Vec<String>,
    env: &Vec<(String, String)>,
    log_path: &PathBuf,
    filesystem: FS,
    process_manager: PM,
) -> Result<CreateProcessOutput, ProviderError>
where
    FS: FileSystem + Send + Sync + 'static,
    PM: ProcessManager + Send + Sync + 'static,
{
    // create process
    let process = process_manager
        .spawn(
            Command::new(program)
                .args(args)
                .envs(env.to_owned())
                .args(args)
                .envs(env.to_owned())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true),
        )
        .await
        .map_err(|err| ProviderError::NodeSpawningFailed(name.to_string(), err.into()))?;
    let stdout = process
        .take_stdout()
        .await
        .expect("infaillible, stdout is piped");
    let stderr = process
        .take_stderr()
        .await
        .expect("Infaillible, stderr is piped");

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
    use std::{ffi::OsString, os::unix::process::ExitStatusExt, process::ExitStatus, str::FromStr};

    use procfs::process::Process;
    use support::{
        fs::in_memory::{InMemoryFile, InMemoryFileSystem},
        process::fake::{DynamicStreamValue, FakeProcessManager, FakeProcessState, StreamValue},
    };
    use tokio::time::timeout;

    use super::*;
    use crate::shared::types::TransferedFile;

    #[test]
    fn provider_capabilities_method_should_return_provider_capabilities() {
        let fs = InMemoryFileSystem::default();
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs, pm);

        let capabilities = provider.capabilities();

        assert_eq!(
            capabilities,
            &ProviderCapabilities {
                requires_image: false
            }
        );
    }

    #[tokio::test]
    async fn provider_tmp_dir_method_should_set_the_temporary_for_provider() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (
                OsString::from_str("/someotherdir").unwrap(),
                InMemoryFile::dir(),
            ),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/someotherdir");

        // we create a namespace to ensure tmp dir will be used to store namespace
        let namespace = provider.create_namespace().await.unwrap();

        assert!(namespace.base_dir().starts_with("/someotherdir"))
    }

    #[tokio::test]
    async fn provider_create_namespace_method_should_create_a_new_namespace_and_returns_it() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone());

        let namespace = provider.create_namespace().await.unwrap();

        // ensure namespace directory is created
        assert!(fs
            .files
            .read()
            .await
            .contains_key(namespace.base_dir().as_os_str()));

        // ensure namespace is added to provider namespaces
        assert_eq!(provider.namespaces().await.len(), 1);

        // ensure the only provider namespace is the same one as the one we just created
        assert!(provider.namespaces().await.get(namespace.id()).is_some());
    }

    #[tokio::test]
    async fn provider_namespaces_method_should_return_empty_namespaces_map_if_the_provider_has_no_namespaces(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone());

        assert_eq!(provider.namespaces().await.len(), 0);
    }

    #[tokio::test]
    async fn provider_namespaces_method_should_return_filled_namespaces_map_if_the_provider_has_one_namespace(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone());

        let namespace = provider.create_namespace().await.unwrap();

        assert_eq!(provider.namespaces().await.len(), 1);
        assert!(provider.namespaces().await.get(namespace.id()).is_some());
    }

    #[tokio::test]
    async fn provider_namespaces_method_should_return_filled_namespaces_map_if_the_provider_has_two_namespaces(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm);

        let namespace1 = provider.create_namespace().await.unwrap();
        let namespace2 = provider.create_namespace().await.unwrap();

        assert_eq!(provider.namespaces().await.len(), 2);
        assert!(provider.namespaces().await.get(namespace1.id()).is_some());
        assert!(provider.namespaces().await.get(namespace2.id()).is_some());
    }

    #[tokio::test]
    async fn namespace_spawn_node_method_should_creates_a_new_node_correctly() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
            (
                OsString::from_str("/file1").unwrap(),
                InMemoryFile::file("My file 1"),
            ),
            (
                OsString::from_str("/file2").unwrap(),
                InMemoryFile::file("My file 2"),
            ),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        let node = namespace
            .spawn_node(
                SpawnNodeOptions::new("mynode", "/path/to/my/node_binary")
                    .args(vec![
                        "-flag1",
                        "--flag2",
                        "--option1=value1",
                        "-option2=value2",
                        "--option3 value3",
                        "-option4 value4",
                    ])
                    .env(vec![
                        ("MY_VAR_1", "MY_VALUE_1"),
                        ("MY_VAR_2", "MY_VALUE_2"),
                        ("MY_VAR_3", "MY_VALUE_3"),
                    ])
                    .injected_files(vec![
                        TransferedFile::new("/file1", "/cfg/file1"),
                        TransferedFile::new("/file2", "/data/file2"),
                    ]),
            )
            .await
            .unwrap();

        // ensure node directories are created
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.base_dir().as_os_str()));
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.config_dir().as_os_str()));
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.data_dir().as_os_str()));
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.scripts_dir().as_os_str()));

        // ensure injected files are presents
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!("{}/file1", node.config_dir().to_string_lossy()))
                        .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 1"
        );
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!("{}/file2", node.data_dir().to_string_lossy()))
                        .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 2"
        );

        // ensure only one process exists
        assert_eq!(pm.count().await, 1);

        // retrieve the process
        let processes = pm.processes().await;
        let process = processes.first().unwrap();

        // ensure process has correct state
        assert!(matches!(process.state().await, FakeProcessState::Running));

        // ensure process is passed correct args
        assert!(process
            .args
            .contains(&OsString::from_str("-flag1").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--flag2").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--option1=value1").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("-option2=value2").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--option3 value3").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("-option4 value4").unwrap()));

        // ensure process has correct environment
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_1").unwrap(),
            OsString::from_str("MY_VALUE_1").unwrap()
        )));
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_2").unwrap(),
            OsString::from_str("MY_VALUE_2").unwrap()
        )));
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_3").unwrap(),
            OsString::from_str("MY_VALUE_3").unwrap()
        )));

        // ensure log file is created and logs are written and keep being written for some time
        pm.advance_by(1).await;
        let expected = ["Line 1\n", "Line 1\nLine 2\n", "Line 1\nLine 2\nLine 3\n"];
        let mut index = 0;
        timeout(Duration::from_secs(3), async {
            loop {
                // if we reach the expected len, all logs have been emited correctly in order
                if index == expected.len() {
                    break;
                }

                // check if there is some existing file with contents
                if let Some(contents) = fs
                    .files
                    .read()
                    .await
                    .get(node.log_path().as_os_str())
                    .map(|file| file.contents().unwrap())
                {
                    // if the contents correspond to what we expect, we continue to check the next expected thing and simulate cpu cycle
                    if contents == expected[index] {
                        index += 1;
                        pm.advance_by(1).await;
                    }
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // ensure node is present in namespace
        assert_eq!(namespace.nodes().await.len(), 1);
        assert!(namespace.nodes().await.get(node.name()).is_some());
    }

    #[tokio::test]
    async fn namespace_spawn_node_method_should_returns_an_error_if_a_node_already_exists_with_this_name(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm);
        let namespace = provider.create_namespace().await.unwrap();

        namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "./testing/dummy_node"))
            .await
            .unwrap();

        let result = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "./testing/dummy_node"))
            .await;

        // we must match here because Arc<dyn Node + Send + Sync> doesn't implements Debug, so unwrap_err is not an option
        match result {
            Ok(_) => unreachable!(),
            Err(err) => assert_eq!(err.to_string(), "Duplicated node name: mynode"),
        };
    }

    #[tokio::test]
    async fn namespace_spawn_node_method_should_returns_an_error_spawning_process_failed() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // force error
        pm.spawn_should_error(std::io::ErrorKind::TimedOut).await;

        let result = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "./testing/dummy_node"))
            .await;

        // we must match here because Arc<dyn Node + Send + Sync> doesn't implements Debug, so unwrap_err is not an option
        match result {
            Ok(_) => unreachable!(),
            Err(err) => assert_eq!(err.to_string(), "Failed to spawn node 'mynode': timed out"),
        };
    }

    #[tokio::test]
    async fn namespace_generate_files_method_should_create_files_at_the_correct_locations_using_given_commands(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([
            (
                OsString::from_str("echo").unwrap(),
                vec![StreamValue::DynamicStdout(DynamicStreamValue::new(
                    |_, args, _| format!("{}\n", args.first().unwrap().to_string_lossy()),
                ))],
            ),
            (
                OsString::from_str("sh").unwrap(),
                vec![StreamValue::DynamicStdout(DynamicStreamValue::new(
                    |_, _, envs| envs.first().unwrap().1.to_string_lossy().to_string(),
                ))],
            ),
        ]));
        let provider = NativeProvider::new(fs.clone(), pm);
        let namespace = provider.create_namespace().await.unwrap();

        namespace
            .generate_files(GenerateFilesOptions::new(vec![
                GenerateFileCommand::new("echo", "/myfile1").args(vec!["My file 1"]),
                GenerateFileCommand::new("sh", "/myfile2")
                    .args(vec!["-c", "echo -n $MY_CONTENT"])
                    .env(vec![("MY_CONTENT", "My file 2")]),
            ]))
            .await
            .unwrap();

        // ensure files have been generated correctly to right location
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!(
                        "{}/myfile1",
                        namespace.base_dir().to_string_lossy()
                    ))
                    .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 1\n"
        );
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!(
                        "{}/myfile2",
                        namespace.base_dir().to_string_lossy()
                    ))
                    .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 2"
        );

        // ensure temporary node has been destroyed
        assert_eq!(namespace.nodes().await.len(), 0);
    }

    #[tokio::test]
    async fn namespace_destroy_should_destroy_all_namespace_nodes_and_namespace_itself() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn 2 dummy nodes to populate namespace
        namespace
            .spawn_node(SpawnNodeOptions::new("mynode1", "./testing/dummy_node"))
            .await
            .unwrap();
        namespace
            .spawn_node(SpawnNodeOptions::new("mynode2", "./testing/dummy_node"))
            .await
            .unwrap();

        // ensure nodes are presents
        assert_eq!(namespace.nodes().await.len(), 2);

        namespace.destroy().await.unwrap();

        // ensure nodes are destroyed
        assert_eq!(namespace.nodes().await.len(), 0);

        // retrieve running process
        let processes = get_processes_by_name("dummy_node").await;

        // ensure no running process exists
        assert_eq!(processes.len(), 0);

        // ensure namespace is destroyed
        assert_eq!(provider.namespaces().await.len(), 0);
    }

    #[tokio::test]
    async fn node_logs_method_should_return_its_logs_as_a_string() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate logs process manager output
        pm.advance_by(3).await;

        // ensure logs are correct after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 3)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\nLine 3\n");
    }

    #[tokio::test]
    async fn node_dump_logs_method_should_writes_its_logs_to_a_given_destination() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate logs process manager output
        pm.advance_by(3).await;

        // ensure logs are correct after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 3)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // dump logs
        node.dump_logs(PathBuf::from("/tmp/my_log_file"))
            .await
            .unwrap();

        assert_eq!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/tmp/my_log_file").unwrap())
                .unwrap()
                .contents()
                .unwrap(),
            "Line 1\nLine 2\nLine 3\n"
        );
    }

    #[tokio::test]
    async fn node_run_command_method_should_execute_the_command_successfully_and_returns_stdout() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([
            (
                OsString::from_str("/path/to/my/node_binary").unwrap(),
                vec![
                    StreamValue::Stdout("Line 1\n".to_string()),
                    StreamValue::Stdout("Line 2\n".to_string()),
                    StreamValue::Stdout("Line 3\n".to_string()),
                ],
            ),
            (
                OsString::from_str("sh").unwrap(),
                vec![StreamValue::DynamicStdout(DynamicStreamValue::new(
                    |_, _, envs| format!("{}\n", envs.first().unwrap().1.to_string_lossy()),
                ))],
            ),
        ]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        pm.advance_by(3).await;

        let result = node
            .run_command(
                RunCommandOptions::new("sh")
                    .args(vec!["-c", "echo $MY_ENV_VAR"])
                    .env(vec![("MY_ENV_VAR", "Here is my content")]),
            )
            .await;

        assert!(matches!(result, Ok(Ok(stdout)) if stdout == "Here is my content\n"));
    }

    #[tokio::test]
    async fn node_run_command_method_should_execute_the_command_successfully_and_returns_error_code_and_stderr_if_an_error_happened(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([
            (
                OsString::from_str("/path/to/my/node_binary").unwrap(),
                vec![
                    StreamValue::Stdout("Line 1\n".to_string()),
                    StreamValue::Stdout("Line 2\n".to_string()),
                    StreamValue::Stdout("Line 3\n".to_string()),
                ],
            ),
            (
                OsString::from_str("sh").unwrap(),
                vec![StreamValue::Stderr("Some error happened".to_string())],
            ),
        ]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // force error
        pm.output_should_fail(ExitStatus::from_raw(1)).await;

        let result = node
            .run_command(RunCommandOptions::new("sh").args(vec!["-fakeargs"]))
            .await;

        assert!(
            matches!(result, Ok(Err((exit_code, stderr))) if !exit_code.success() && stderr == "Some error happened")
        );
    }

    #[tokio::test]
    async fn node_run_command_method_should_fail_to_execute_the_command_if_command_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // force error
        pm.output_should_error(std::io::ErrorKind::NotFound).await;

        let err = node
            .run_command(RunCommandOptions::new("myrandomprogram"))
            .await
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Error running command 'myrandomprogram': entity not found"
        );
    }

    #[tokio::test]
    async fn node_run_script_method_should_execute_the_script_successfully_and_returns_stdout() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/path").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/path/to").unwrap(), InMemoryFile::dir()),
            (
                OsString::from_str("/path/to/my").unwrap(),
                InMemoryFile::dir(),
            ),
            (
                OsString::from_str("/path/to/my/script").unwrap(),
                InMemoryFile::file("some script"),
            ),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // we need to push stream after node spawn because the final script path is determined by the node local path
        pm.push_stream(
            format!("{}/script", node.scripts_dir().to_string_lossy()).into(),
            vec![
                StreamValue::Stdout("My script\n".to_string()),
                StreamValue::DynamicStdout(DynamicStreamValue::new(|_, _, envs| {
                    format!("{}\n", envs.first().unwrap().1.to_string_lossy())
                })),
                StreamValue::DynamicStdout(DynamicStreamValue::new(|_, args, _| {
                    if args.first().is_some_and(|arg| arg == "-c") {
                        "With args\n".to_string()
                    } else {
                        String::new()
                    }
                })),
            ],
        )
        .await;

        pm.advance_by(3).await;

        let result = node
            .run_script(
                RunScriptOptions::new("/path/to/my/script")
                    .args(vec!["-c"])
                    .env(vec![("MY_ENV_VAR", "With env")]),
            )
            .await;

        assert!(matches!(result, Ok(Ok(stdout)) if stdout == "My script\nWith env\nWith args\n"));
    }

    #[tokio::test]
    async fn run_script_method_should_fails_if_script_doesnt_exists_locally() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate process advancing
        pm.advance_by(3).await;

        let err = node
            .run_script(
                RunScriptOptions::new("/path/to/my/script")
                    .args(vec!["-c"])
                    .env(vec![("MY_ENV_VAR", "With env")]),
            )
            .await
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Script with path /path/to/my/script not found"
        );
    }

    #[tokio::test]
    async fn node_copy_file_from_node_method_should_copy_node_remote_file_to_local_path() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        pm.advance_by(3).await;

        // wait for logs to be written
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 3)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        node.copy_file_from_node(
            PathBuf::from("/mynode.log"),
            PathBuf::from("/nodelog.backup"),
        )
        .await
        .unwrap();

        assert_eq!(
            fs.files.read().await.get(node.log_path().as_os_str()),
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/nodelog.backup").unwrap())
        );
    }

    #[tokio::test]
    async fn node_pause_method_should_pause_the_node_process() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
                StreamValue::Stdout("Line 4\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        {
            // retrieve running process
            let processes = pm.processes().await;
            assert_eq!(processes.len(), 1);
            let node_process = processes.first().unwrap();

            // ensure process has correct state pre-pause
            assert!(matches!(
                node_process.state().await,
                FakeProcessState::Running
            ));

            // simulate logs process manager output
            pm.advance_by(2).await;
        }

        // ensure logs are correct after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 2)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");

        // pause the node
        node.pause().await.unwrap();

        // simulate process manager advancing process when process paused
        {
            // retrieve running process
            let processes = pm.processes().await;
            assert_eq!(processes.len(), 1);
            let node_process = processes.first().unwrap();

            // ensure process has correct state post-pause
            assert!(matches!(
                node_process.state().await,
                FakeProcessState::Stopped
            ));

            pm.advance_by(2).await;
        }

        // ensure logs didn't change after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 2)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");
    }

    #[tokio::test]
    async fn node_pause_method_should_fails_if_some_error_happened() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // force error
        pm.kill_should_error(nix::errno::Errno::EPERM).await;

        // pause the node where some error would happen
        let err = node.pause().await.unwrap_err();

        assert_eq!(err.to_string(), "Failed to pause node 'mynode'");
    }

    #[tokio::test]
    async fn node_resume_method_should_resume_the_paused_node_process() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
                StreamValue::Stdout("Line 4\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        {
            // retrieve running process
            let processes = pm.processes().await;
            assert_eq!(processes.len(), 1);
            let node_process = processes.first().unwrap();

            // ensure process has correct state pre-pause
            assert!(matches!(
                node_process.state().await,
                FakeProcessState::Running
            ));

            // simulate logs process manager output
            pm.advance_by(2).await;
        }

        // ensure logs are correct after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 2)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // ensure logs are correct after some time
        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");

        node.pause().await.unwrap();

        {
            // retrieve running process
            let processes = pm.processes().await;
            assert_eq!(processes.len(), 1);
            let node_process = processes.first().unwrap();

            // ensure process has correct state post-pause / pre-resume
            assert!(matches!(
                node_process.state().await,
                FakeProcessState::Stopped
            ));

            // simulate logs process manager output
            pm.advance_by(2).await;
        }

        // ensure logs are not written when process is paused
        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");

        node.resume().await.unwrap();

        {
            // retrieve running process
            let processes = pm.processes().await;
            assert_eq!(processes.len(), 1);
            let node_process = processes.first().unwrap();

            // ensure process has correct state post-resume
            assert!(matches!(
                node_process.state().await,
                FakeProcessState::Running
            ));

            // simulate logs process manager output
            pm.advance_by(2).await;
        }

        // ensure logs are correct after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 4)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // ensure logs are written and correct after process is resumed
        assert_eq!(
            node.logs().await.unwrap(),
            "Line 1\nLine 2\nLine 3\nLine 4\n"
        );
    }

    #[tokio::test]
    async fn node_resume_method_should_fails_if_some_error_happened() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // pause the node
        node.pause().await.unwrap();

        // force error
        pm.kill_should_error(nix::errno::Errno::EPERM).await;

        let err = node.resume().await.unwrap_err();

        assert_eq!(err.to_string(), "Failed to resume node 'mynode'");
    }

    #[tokio::test]
    async fn node_restart_should_kill_the_node_and_respawn_it_successfully() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
            (
                OsString::from_str("/file1").unwrap(),
                InMemoryFile::file("My file 1"),
            ),
            (
                OsString::from_str("/file2").unwrap(),
                InMemoryFile::file("My file 2"),
            ),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
                StreamValue::Stdout("Line 4\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        let node = namespace
            .spawn_node(
                SpawnNodeOptions::new("mynode", "/path/to/my/node_binary")
                    .args(vec![
                        "-flag1",
                        "--flag2",
                        "--option1=value1",
                        "-option2=value2",
                        "--option3 value3",
                        "-option4 value4",
                    ])
                    .env(vec![
                        ("MY_VAR_1", "MY_VALUE_1"),
                        ("MY_VAR_2", "MY_VALUE_2"),
                        ("MY_VAR_3", "MY_VALUE_3"),
                    ])
                    .injected_files(vec![
                        TransferedFile::new("/file1", "/cfg/file1"),
                        TransferedFile::new("/file2", "/data/file2"),
                    ]),
            )
            .await
            .unwrap();

        let old_process_id = {
            // retrieve running process
            let processes = pm.processes().await;
            assert_eq!(processes.len(), 1);
            let node_process = processes.first().unwrap();

            // ensure process has correct state post-pause / pre-resume
            assert!(matches!(
                node_process.state().await,
                FakeProcessState::Running
            ));

            // simulate process advance and logs writting
            pm.advance_by(2).await;

            node_process.id
        };

        // ensure logs are correct after some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 2)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");

        // restart node
        node.restart(None).await.unwrap();

        // retrieve running process
        let processes = pm.processes().await;
        assert_eq!(processes.len(), 1);
        let process = processes.first().unwrap();

        // ensure process has correct state post-restart
        assert!(matches!(process.state().await, FakeProcessState::Running));

        // simulate process advance and logs writting
        pm.advance_by(2).await;

        // ensure pid changed
        assert_ne!(old_process_id, process.id);

        // ensure process is passed correct args after restart
        assert!(process
            .args
            .contains(&OsString::from_str("-flag1").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--flag2").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--option1=value1").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("-option2=value2").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--option3 value3").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("-option4 value4").unwrap()));

        // ensure process has correct environment after restart
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_1").unwrap(),
            OsString::from_str("MY_VALUE_1").unwrap()
        )));
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_2").unwrap(),
            OsString::from_str("MY_VALUE_2").unwrap()
        )));
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_3").unwrap(),
            OsString::from_str("MY_VALUE_3").unwrap()
        )));

        // ensure logs are correct after restart, appending to old logs or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 4)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(
            node.logs().await.unwrap(),
            "Line 1\nLine 2\nLine 1\nLine 2\n"
        );

        // ensure node is present in namespace
        assert_eq!(namespace.nodes().await.len(), 1);
        assert!(namespace.nodes().await.get(node.name()).is_some());
    }

    #[tokio::test]
    async fn node_restart_method_should_fails_if_some_error_happened() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // force error
        pm.node_kill_should_error(nix::errno::Errno::EPERM).await;

        let err = node.restart(None).await.unwrap_err();

        assert_eq!(err.to_string(), "Failed to kill node 'mynode'");
    }

    #[tokio::test]
    async fn node_destroy_method_should_destroy_the_node_itfself_and_remove_process_and_stop_logs_writing(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
                StreamValue::Stdout("Line 4\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate process advancing
        pm.advance_by(2).await;

        // ensure logs are correct, waiting some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 2)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");

        // destroy the node
        node.destroy().await.unwrap();

        // simulate processes advancing
        pm.advance_by(2).await;

        // ensure logs are not being written anymore, waiting some time or timeout
        timeout(Duration::from_secs(3), async {
            loop {
                if node
                    .logs()
                    .await
                    .is_ok_and(|logs| logs.lines().count() == 2)
                {
                    return;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(node.logs().await.unwrap(), "Line 1\nLine 2\n");

        // ensure process is not running anymore
        assert_eq!(pm.processes().await.len(), 0);

        // ensure node doesn't exists anymore in namespace
        assert_eq!(namespace.nodes().await.len(), 0);
    }

    #[tokio::test]
    async fn node_destroy_method_should_fails_if_some_error_happened() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone());
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // force error
        pm.node_kill_should_error(nix::errno::Errno::EPERM).await;

        let err = node.destroy().await.unwrap_err();

        assert_eq!(err.to_string(), "Failed to kill node 'mynode'");
    }

    async fn get_processes_by_name(name: &str) -> Vec<Process> {
        procfs::process::all_processes()
            .unwrap()
            .filter_map(|process| {
                if let Ok(process) = process {
                    process
                        .cmdline()
                        .iter()
                        .any(|args| args.iter().any(|arg| arg.contains(name)))
                        .then_some(process)
                } else {
                    None
                }
            })
            .collect::<Vec<Process>>()
    }
}
