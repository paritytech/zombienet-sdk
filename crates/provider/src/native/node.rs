use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use nix::{sys::signal::Signal, unistd::Pid};
use support::{
    fs::FileSystem,
    process::{Command, DynProcess, ProcessManager},
};
use tokio::{sync::RwLock, task::JoinHandle, time::sleep};

use super::{helpers::create_process_with_log_tasks, namespace::WeakNativeNamespace};
use crate::{
    constants::LOCALHOST,
    types::{ExecutionResult, Port, RunCommandOptions, RunScriptOptions},
    ProviderError, ProviderNode,
};

#[derive(Clone)]
pub(super) struct NativeNode<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub(super) name: String,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) env: Vec<(String, String)>,
    pub(super) base_dir: PathBuf,
    pub(super) config_dir: PathBuf,
    pub(super) data_dir: PathBuf,
    pub(super) scripts_dir: PathBuf,
    pub(super) log_path: PathBuf,
    pub(super) inner: Arc<RwLock<NativeNodeInner>>,
    pub(super) filesystem: FS,
    pub(super) process_manager: PM,
    pub(super) namespace: WeakNativeNamespace<FS, PM>,
}

pub(super) struct NativeNodeInner {
    pub(super) process: DynProcess,
    pub(super) stdout_reading_handle: JoinHandle<()>,
    pub(super) stderr_reading_handle: JoinHandle<()>,
    pub(super) log_writing_handle: JoinHandle<()>,
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

    fn program(&self) -> &str {
        &self.program
    }

    fn args(&self) -> Vec<&str> {
        self.args.iter().map(|arg| arg.as_str()).collect()
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

    fn path_in_node(&self, file: &Path) -> PathBuf {
        let full_path = format!(
            "{}/{}",
            self.base_dir.to_string_lossy(),
            file.to_string_lossy()
        );
        PathBuf::from(full_path)
    }

    async fn ip(&self) -> Result<IpAddr, ProviderError> {
        Ok(LOCALHOST)
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
        // REMOVE
        // let args: Vec<String> = options.args.into_iter().map(|s| s.replace("{{NODE_BASE_PATH}}", self.base_dir().to_string_lossy().as_ref())).collect();
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

    async fn send_file(
        &self,
        _local_src: &PathBuf,
        _remote_dest: &PathBuf,
        _mode: &str,
    ) -> Result<(), ProviderError> {
        // TODO: implement
        Ok(())
    }

    async fn receive_file(
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
            .map_err(|err| ProviderError::PauseNodeFailed(self.name.clone(), err.into()))?;

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;
        let pid = retrieve_pid_from_process(&inner.process, &self.name).await?;

        self.process_manager
            .kill(pid, Signal::SIGCONT)
            .await
            .map_err(|err| ProviderError::ResumeNodeFailed(self.name.clone(), err.into()))?;

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
            .map_err(|err| ProviderError::KillNodeFailed(self.name.clone(), err.into()))?;

        // re-spawn process with tasks for logs
        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &self.name,
                &self.program,
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
            .map_err(|err| ProviderError::KillNodeFailed(self.name.clone(), err.into()))?;

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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap, ffi::OsString, os::unix::process::ExitStatusExt, process::ExitStatus,
        str::FromStr,
    };

    use support::{
        fs::in_memory::{InMemoryFile, InMemoryFileSystem},
        process::fake::{DynamicStreamValue, FakeProcessManager, FakeProcessState, StreamValue},
    };
    use tokio::time::timeout;

    use super::*;
    use crate::{
        native::provider::NativeProvider,
        types::{SpawnNodeOptions, TransferedFile},
        Provider,
    };

    #[tokio::test]
    async fn logs_should_return_its_logs_as_a_string() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn dump_logs_should_writes_its_logs_to_a_given_destination() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn run_command_should_execute_the_command_successfully_and_returns_stdout() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn run_command_should_execute_the_command_successfully_and_returns_error_code_and_stderr_if_an_error_happened(
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn run_command_should_fail_to_execute_the_command_if_command_doesnt_exists() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn run_script_should_execute_the_script_successfully_and_returns_stdout() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn run_script_should_fails_if_script_doesnt_exists_locally() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn copy_file_from_node_should_copy_node_remote_file_to_local_path() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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

        node.receive_file(
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
    async fn pause_should_pause_the_node_process() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn pause_should_fails_if_some_error_happened() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // force error
        pm.kill_should_error(nix::errno::Errno::EPERM).await;

        // pause the node where some error would happen
        let err = node.pause().await.unwrap_err();

        // TODO: actual output on mac "Failed to pause node 'mynode': EPERM: Operation not permitted"
        assert!(err.to_string().contains("Failed to pause node 'mynode'"));
    }

    #[tokio::test]
    async fn resume_should_resume_the_paused_node_process() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn resume_should_fails_if_some_error_happened() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // pause the node
        node.pause().await.unwrap();

        // force error
        pm.kill_should_error(nix::errno::Errno::EPERM).await;

        let err = node.resume().await.unwrap_err();

        // TODO: actual output on mac "Failed to resume node 'mynode': EPERM: Operation not permitted"
        assert!(err.to_string().contains("Failed to resume node 'mynode'"));
    }

    #[tokio::test]
    async fn restart_should_kill_the_node_and_respawn_it_successfully() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        let node = namespace
            .spawn_node(
                &SpawnNodeOptions::new("mynode", "/path/to/my/node_binary")
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
    async fn restart_should_fails_if_some_error_happened() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // force error
        pm.node_kill_should_error(nix::errno::Errno::EPERM).await;

        let err = node.restart(None).await.unwrap_err();

        // TODO: on mac the actual output is "Failed to kill node 'mynode': Operation not permitted (os error 1)"
        assert!(err.to_string().contains("Failed to kill node 'mynode'"))
    }

    #[tokio::test]
    async fn destroy_should_destroy_the_node_itfself_and_remove_process_and_stop_logs_writing() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
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
    async fn destroy_should_fails_if_some_error_happened() {
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
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn dummy node
        let node = namespace
            .spawn_node(&SpawnNodeOptions::new("mynode", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // simulate processes advancing
        pm.advance_by(3).await;

        // force error
        pm.node_kill_should_error(nix::errno::Errno::EPERM).await;

        let err = node.destroy().await.unwrap_err();

        // TODO: on mac the actual output is "Failed to kill node 'mynode': Operation not permitted (os error 1)"
        assert!(err.to_string().contains("Failed to kill node 'mynode'"));
    }
}
