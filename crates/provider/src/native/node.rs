use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::{shared::constants::THIS_IS_A_BUG, types::AssetLocation};
use flate2::read::GzDecoder;
use futures::future::try_join_all;
use nix::{
    sys::signal::{kill, Signal},
    unistd::Pid,
};
use sha2::Digest;
use support::fs::FileSystem;
use tar::Archive;
use tokio::{
    io::{AsyncRead, AsyncReadExt, BufReader},
    process::{Child, ChildStderr, ChildStdout, Command},
    sync::{
        mpsc::{self, Sender},
        RwLock,
    },
    task::JoinHandle,
    time::sleep,
    try_join,
};
use tracing::trace;

use super::namespace::NativeNamespace;
use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, NODE_SCRIPTS_DIR},
    types::{ExecutionResult, RunCommandOptions, RunScriptOptions, TransferedFile},
    ProviderError, ProviderNamespace, ProviderNode,
};

pub(super) struct NativeNode<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    namespace: Weak<NativeNamespace<FS>>,
    name: String,
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    relay_data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    process: RwLock<Option<Child>>,
    stdout_reading_task: RwLock<Option<JoinHandle<()>>>,
    stderr_reading_task: RwLock<Option<JoinHandle<()>>>,
    log_writing_task: RwLock<Option<JoinHandle<()>>>,
    filesystem: FS,
}

impl<FS> NativeNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn new(
        namespace: &Weak<NativeNamespace<FS>>,
        namespace_base_dir: &PathBuf,
        name: &str,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        startup_files: &[TransferedFile],
        created_paths: &[PathBuf],
        db_snapshot: &Option<&AssetLocation>,
        filesystem: &FS,
    ) -> Result<Arc<Self>, ProviderError> {
        let base_dir = PathBuf::from_iter([namespace_base_dir, &PathBuf::from(name)]);
        trace!("creating base_dir {:?}", base_dir);
        filesystem.create_dir_all(&base_dir).await?;
        trace!("created base_dir {:?}", base_dir);

        let base_dir_raw = base_dir.to_string_lossy();
        let config_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_DATA_DIR));
        let relay_data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_RELAY_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_SCRIPTS_DIR));
        let log_path = base_dir.join(format!("{name}.log"));

        trace!("creating dirs {:?}", config_dir);
        try_join!(
            filesystem.create_dir(&config_dir),
            filesystem.create_dir(&data_dir),
            filesystem.create_dir(&relay_data_dir),
            filesystem.create_dir(&scripts_dir),
        )?;
        trace!("created!");

        let node = Arc::new(NativeNode {
            namespace: namespace.clone(),
            name: name.to_string(),
            program: program.to_string(),
            args: args.to_vec(),
            env: env.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            process: RwLock::new(None),
            stdout_reading_task: RwLock::new(None),
            stderr_reading_task: RwLock::new(None),
            log_writing_task: RwLock::new(None),
            filesystem: filesystem.clone(),
        });

        node.initialize_startup_paths(created_paths).await?;
        node.initialize_startup_files(startup_files).await?;

        if let Some(db_snap) = db_snapshot {
            node.initialize_db_snapshot(db_snap).await?;
        }

        let (stdout, stderr) = node.initialize_process().await?;

        node.initialize_log_writing(stdout, stderr).await;

        Ok(node)
    }

    async fn initialize_startup_paths(&self, paths: &[PathBuf]) -> Result<(), ProviderError> {
        trace!("creating paths {:?}", paths);
        let base_dir_raw = self.base_dir.to_string_lossy();
        try_join_all(paths.iter().map(|file| {
            let full_path = format!("{base_dir_raw}{}", file.to_string_lossy());
            self.filesystem.create_dir_all(full_path)
        }))
        .await?;
        trace!("paths created!");

        Ok(())
    }

    async fn initialize_startup_files(
        &self,
        startup_files: &[TransferedFile],
    ) -> Result<(), ProviderError> {
        trace!("creating files {:?}", startup_files);
        try_join_all(
            startup_files
                .iter()
                .map(|file| self.send_file(&file.local_path, &file.remote_path, &file.mode)),
        )
        .await?;
        trace!("files created!");

        Ok(())
    }

    async fn initialize_db_snapshot(
        &self,
        db_snapshot: &AssetLocation,
    ) -> Result<(), ProviderError> {
        trace!("snap: {db_snapshot}");

        // check if we need to get the db or is already in the ns
        let ns_base_dir = self.namespace_base_dir();
        let hashed_location = match db_snapshot {
            AssetLocation::Url(location) => hex::encode(sha2::Sha256::digest(location.to_string())),
            AssetLocation::FilePath(filepath) => {
                hex::encode(sha2::Sha256::digest(filepath.to_string_lossy().to_string()))
            },
        };

        let full_path = format!("{}/{}.tgz", ns_base_dir, hashed_location);
        trace!("db_snap fullpath in ns: {full_path}");
        if !self.filesystem.exists(&full_path).await {
            // needs to download/copy
            self.get_db_snapshot(db_snapshot, &full_path).await?;
        }

        let contents = self.filesystem.read(full_path).await.unwrap();
        let gz = GzDecoder::new(&contents[..]);
        let mut archive = Archive::new(gz);
        archive
            .unpack(self.base_dir.to_string_lossy().as_ref())
            .unwrap();

        Ok(())
    }

    async fn get_db_snapshot(
        &self,
        location: &AssetLocation,
        full_path: &str,
    ) -> Result<(), ProviderError> {
        trace!("getting db_snapshot from: {:?} to: {full_path}", location);
        match location {
            AssetLocation::Url(location) => {
                let res = reqwest::get(location.as_ref())
                    .await
                    .map_err(|err| ProviderError::DownloadFile(location.to_string(), err.into()))?;

                let contents: &[u8] = &res.bytes().await.unwrap();
                trace!("writing: {full_path}");
                self.filesystem.write(full_path, contents).await?;
            },
            AssetLocation::FilePath(filepath) => {
                self.filesystem.copy(filepath, full_path).await?;
            },
        };

        Ok(())
    }

    async fn initialize_process(&self) -> Result<(ChildStdout, ChildStderr), ProviderError> {
        let mut process = Command::new(&self.program)
            .args(&self.args)
            .envs(self.env.to_vec())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .current_dir(&self.base_dir)
            .spawn()
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;
        let stdout = process
            .stdout
            .take()
            .expect(&format!("infaillible, stdout is piped {THIS_IS_A_BUG}"));
        let stderr = process
            .stderr
            .take()
            .expect(&format!("infaillible, stderr is piped {THIS_IS_A_BUG}"));

        self.process.write().await.replace(process);

        Ok((stdout, stderr))
    }

    async fn initialize_log_writing(&self, stdout: ChildStdout, stderr: ChildStderr) {
        let (stdout_tx, mut rx) = mpsc::channel(10);
        let stderr_tx = stdout_tx.clone();

        self.stdout_reading_task
            .write()
            .await
            .replace(self.create_stream_polling_task(stdout, stdout_tx));
        self.stderr_reading_task
            .write()
            .await
            .replace(self.create_stream_polling_task(stderr, stderr_tx));

        let filesystem = self.filesystem.clone();
        let log_path = self.log_path.clone();

        self.log_writing_task
            .write()
            .await
            .replace(tokio::spawn(async move {
                loop {
                    while let Some(Ok(data)) = rx.recv().await {
                        // TODO: find a better way instead of ignoring error ?
                        let _ = filesystem.append(&log_path, data).await;
                    }
                    sleep(Duration::from_millis(250)).await;
                }
            }));
    }

    fn create_stream_polling_task(
        &self,
        stream: impl AsyncRead + Unpin + Send + 'static,
        tx: Sender<Result<Vec<u8>, std::io::Error>>,
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

    async fn process_id(&self) -> Result<Pid, ProviderError> {
        let raw_pid = self
            .process
            .read()
            .await
            .as_ref()
            .and_then(|process| process.id())
            .ok_or_else(|| ProviderError::ProcessIdRetrievalFailed(self.name.to_string()))?;

        Ok(Pid::from_raw(raw_pid as i32))
    }

    async fn abort(&self) -> anyhow::Result<()> {
        self.log_writing_task
            .write()
            .await
            .take()
            .ok_or_else(|| anyhow!("no log writing task was attached for the node"))?
            .abort();

        self.stdout_reading_task
            .write()
            .await
            .take()
            .ok_or_else(|| anyhow!("no stdout reading task was attached for the node"))?
            .abort();

        self.stderr_reading_task
            .write()
            .await
            .take()
            .ok_or_else(|| anyhow!("no stderr reading task was attached for the node"))?
            .abort();

        self.process
            .write()
            .await
            .take()
            .ok_or_else(|| anyhow!("no process was attached for the node"))?
            .kill()
            .await?;

        Ok(())
    }

    fn namespace_base_dir(&self) -> String {
        self.namespace
            .upgrade()
            .map(|namespace| namespace.base_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| panic!("namespace shouldn't be dropped, {}", THIS_IS_A_BUG))
    }
}

#[async_trait]
impl<FS> ProviderNode for NativeNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
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

    fn relay_data_dir(&self) -> &PathBuf {
        &self.relay_data_dir
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
        let result = Command::new(options.program.clone())
            .args(options.args.clone())
            .envs(options.env)
            .current_dir(&self.base_dir)
            .output()
            .await
            .map_err(|err| {
                ProviderError::RunCommandError(
                    format!("{} {}", &options.program, &options.args.join(" ")),
                    options.program,
                    err.into(),
                )
            })?;

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
        local_file_path: &Path,
        remote_file_path: &Path,
        mode: &str,
    ) -> Result<(), ProviderError> {
        let namespaced_remote_file_path = PathBuf::from(format!(
            "{}{}",
            &self.base_dir.to_string_lossy(),
            remote_file_path.to_string_lossy()
        ));

        self.filesystem
            .copy(local_file_path, &namespaced_remote_file_path)
            .await?;

        self.run_command(
            RunCommandOptions::new("chmod")
                .args(vec![mode, &namespaced_remote_file_path.to_string_lossy()]),
        )
        .await?
        .map_err(|(_, err)| ProviderError::SendFile(self.name.clone(), anyhow!("{err}")))?;

        Ok(())
    }

    async fn receive_file(
        &self,
        remote_file_path: &Path,
        local_file_path: &Path,
    ) -> Result<(), ProviderError> {
        let namespaced_remote_file_path = PathBuf::from(format!(
            "{}{}",
            &self.base_dir.to_string_lossy(),
            remote_file_path.to_string_lossy()
        ));

        self.filesystem
            .copy(namespaced_remote_file_path, local_file_path)
            .await?;

        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        let process_id = self.process_id().await?;

        kill(process_id, Signal::SIGSTOP)
            .map_err(|err| ProviderError::PauseNodeFailed(self.name.clone(), err.into()))?;

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        let process_id = self.process_id().await?;

        nix::sys::signal::kill(process_id, Signal::SIGCONT)
            .map_err(|err| ProviderError::ResumeNodeFailed(self.name.clone(), err.into()))?;

        Ok(())
    }

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError> {
        if let Some(duration) = after {
            sleep(duration).await;
        }

        self.abort()
            .await
            .map_err(|err| ProviderError::RestartNodeFailed(self.name.clone(), err))?;

        let (stdout, stderr) = self
            .initialize_process()
            .await
            .map_err(|err| ProviderError::RestartNodeFailed(self.name.clone(), err.into()))?;

        self.initialize_log_writing(stdout, stderr).await;

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        self.abort()
            .await
            .map_err(|err| ProviderError::DestroyNodeFailed(self.name.clone(), err))?;

        if let Some(namespace) = self.namespace.upgrade() {
            namespace.nodes.write().await.remove(&self.name);
        }

        Ok(())
    }
}
