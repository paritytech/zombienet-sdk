use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::types::AssetLocation;
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use support::{constants::THIS_IS_A_BUG, fs::FileSystem};
use tokio::{time::sleep, try_join};
use tracing::debug;

use super::{
    client::{ContainerRunOptions, DockerClient},
    namespace::DockerNamespace,
};
use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, NODE_SCRIPTS_DIR},
    docker,
    types::{ExecutionResult, Port, RunCommandOptions, RunScriptOptions, TransferedFile},
    ProviderError, ProviderNamespace, ProviderNode,
};

pub(super) struct DockerNodeOptions<'a, FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) namespace: &'a Weak<DockerNamespace<FS>>,
    pub(super) namespace_base_dir: &'a PathBuf,
    pub(super) name: &'a str,
    pub(super) image: Option<&'a String>,
    pub(super) program: &'a str,
    pub(super) args: &'a [String],
    pub(super) env: &'a [(String, String)],
    pub(super) startup_files: &'a [TransferedFile],
    pub(super) db_snapshot: Option<&'a AssetLocation>,
    pub(super) docker_client: &'a DockerClient,
    pub(super) container_name: String,
    pub(super) filesystem: &'a FS,
    pub(super) port_mapping: &'a HashMap<Port, Port>,
}

impl<'a, FS> DockerNodeOptions<'a, FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub fn from_deserializable(
        deserializable: &'a DeserializableDockerNodeOptions,
        namespace: &'a Weak<DockerNamespace<FS>>,
        namespace_base_dir: &'a PathBuf,
        docker_client: &'a DockerClient,
        filesystem: &'a FS,
    ) -> Self {
        DockerNodeOptions {
            namespace,
            namespace_base_dir,
            name: &deserializable.name,
            image: deserializable.image.as_ref(),
            program: &deserializable.program,
            args: &deserializable.args,
            env: &deserializable.env,
            startup_files: &[],
            db_snapshot: None,
            docker_client,
            container_name: deserializable.container_name.clone(),
            filesystem,
            port_mapping: &deserializable.port_mapping,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct DeserializableDockerNodeOptions {
    pub(super) name: String,
    pub(super) image: Option<String>,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) env: Vec<(String, String)>,
    pub(super) container_name: String,
    pub(super) port_mapping: HashMap<Port, Port>,
}

#[derive(Serialize)]
pub struct DockerNode<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    #[serde(skip)]
    namespace: Weak<DockerNamespace<FS>>,
    name: String,
    image: String,
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    relay_data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    #[serde(skip)]
    docker_client: DockerClient,
    container_name: String,
    port_mapping: HashMap<Port, Port>,
    #[allow(dead_code)]
    #[serde(skip)]
    filesystem: FS,
    provider_tag: String,
}

impl<FS> DockerNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        options: DockerNodeOptions<'_, FS>,
    ) -> Result<Arc<Self>, ProviderError> {
        let image = options.image.ok_or_else(|| {
            ProviderError::MissingNodeInfo(options.name.to_string(), "missing image".to_string())
        })?;

        let filesystem = options.filesystem.clone();

        let base_dir =
            PathBuf::from_iter([options.namespace_base_dir, &PathBuf::from(options.name)]);
        filesystem.create_dir_all(&base_dir).await?;

        let base_dir_raw = base_dir.to_string_lossy();
        let config_dir = PathBuf::from(format!("{base_dir_raw}{NODE_CONFIG_DIR}"));
        let data_dir = PathBuf::from(format!("{base_dir_raw}{NODE_DATA_DIR}"));
        let relay_data_dir = PathBuf::from(format!("{base_dir_raw}{NODE_RELAY_DATA_DIR}"));
        let scripts_dir = PathBuf::from(format!("{base_dir_raw}{NODE_SCRIPTS_DIR}"));
        let log_path = base_dir.join("node.log");

        try_join!(
            filesystem.create_dir_all(&config_dir),
            filesystem.create_dir_all(&data_dir),
            filesystem.create_dir_all(&relay_data_dir),
            filesystem.create_dir_all(&scripts_dir),
        )?;

        let node = Arc::new(DockerNode {
            namespace: options.namespace.clone(),
            name: options.name.to_string(),
            image: image.to_string(),
            program: options.program.to_string(),
            args: options.args.to_vec(),
            env: options.env.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            docker_client: options.docker_client.clone(),
            container_name: options.container_name,
            port_mapping: options.port_mapping.clone(),
            provider_tag: docker::provider::PROVIDER_NAME.to_string(),
        });

        node.initialize_docker().await?;

        if let Some(db_snap) = options.db_snapshot {
            node.initialize_db_snapshot(db_snap).await?;
        }

        node.initialize_startup_files(options.startup_files).await?;

        node.start().await?;

        Ok(node)
    }

    pub(super) async fn attach_to_live(
        options: DockerNodeOptions<'_, FS>,
    ) -> Result<Arc<Self>, ProviderError> {
        let image = options.image.ok_or_else(|| {
            ProviderError::MissingNodeInfo(options.name.to_string(), "missing image".to_string())
        })?;

        let filesystem = options.filesystem.clone();

        let base_dir =
            PathBuf::from_iter([options.namespace_base_dir, &PathBuf::from(options.name)]);
        filesystem.create_dir_all(&base_dir).await?;

        let base_dir_raw = base_dir.to_string_lossy();
        let config_dir = PathBuf::from(format!("{base_dir_raw}{NODE_CONFIG_DIR}"));
        let data_dir = PathBuf::from(format!("{base_dir_raw}{NODE_DATA_DIR}"));
        let relay_data_dir = PathBuf::from(format!("{base_dir_raw}{NODE_RELAY_DATA_DIR}"));
        let scripts_dir = PathBuf::from(format!("{base_dir_raw}{NODE_SCRIPTS_DIR}"));
        let log_path = base_dir.join("node.log");

        let node = Arc::new(DockerNode {
            namespace: options.namespace.clone(),
            name: options.name.to_string(),
            image: image.to_string(),
            program: options.program.to_string(),
            args: options.args.to_vec(),
            env: options.env.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            docker_client: options.docker_client.clone(),
            container_name: options.container_name,
            port_mapping: options.port_mapping.clone(),
            provider_tag: docker::provider::PROVIDER_NAME.to_string(),
        });

        Ok(node)
    }

    async fn initialize_docker(&self) -> Result<(), ProviderError> {
        let command = [vec![self.program.to_string()], self.args.to_vec()].concat();

        self.docker_client
            .container_run(
                ContainerRunOptions::new(&self.image, command)
                    .name(&self.container_name)
                    .env(self.env.clone())
                    .volume_mounts(HashMap::from([
                        (
                            format!("{}-zombie-wrapper", self.namespace_name(),),
                            "/scripts".to_string(),
                        ),
                        (
                            format!("{}-helper-binaries", self.namespace_name()),
                            "/helpers".to_string(),
                        ),
                        (
                            self.config_dir.to_string_lossy().into_owned(),
                            "/cfg".to_string(),
                        ),
                        (
                            self.data_dir.to_string_lossy().into_owned(),
                            "/data".to_string(),
                        ),
                        (
                            self.relay_data_dir.to_string_lossy().into_owned(),
                            "/relay-data".to_string(),
                        ),
                    ]))
                    .entrypoint("/scripts/zombie-wrapper.sh")
                    .port_mapping(&self.port_mapping),
            )
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err.into()))?;

        // change dirs permission
        let _ = self
            .docker_client
            .container_exec(
                &self.container_name,
                ["chmod", "777", "/cfg", "/data", "/relay-data"].into(),
                None,
                Some("root"),
            )
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err.into()))?;

        Ok(())
    }

    async fn initialize_db_snapshot(
        &self,
        _db_snapshot: &AssetLocation,
    ) -> Result<(), ProviderError> {
        todo!()
        // trace!("snap: {db_snapshot}");
        // let url_of_snap = match db_snapshot {
        //     AssetLocation::Url(location) => location.clone(),
        //     AssetLocation::FilePath(filepath) => self.upload_to_fileserver(filepath).await?,
        // };

        // // we need to get the snapshot from a public access
        // // and extract to /data
        // let opts = RunCommandOptions::new("mkdir").args([
        //     "-p",
        //     "/data/",
        //     "&&",
        //     "mkdir",
        //     "-p",
        //     "/relay-data/",
        //     "&&",
        //     // Use our version of curl
        //     "/cfg/curl",
        //     url_of_snap.as_ref(),
        //     "--output",
        //     "/data/db.tgz",
        //     "&&",
        //     "cd",
        //     "/",
        //     "&&",
        //     "tar",
        //     "--skip-old-files",
        //     "-xzvf",
        //     "/data/db.tgz",
        // ]);

        // trace!("cmd opts: {:#?}", opts);
        // let _ = self.run_command(opts).await?;

        // Ok(())
    }

    async fn initialize_startup_files(
        &self,
        startup_files: &[TransferedFile],
    ) -> Result<(), ProviderError> {
        try_join_all(
            startup_files
                .iter()
                .map(|file| self.send_file(&file.local_path, &file.remote_path, &file.mode)),
        )
        .await?;

        Ok(())
    }

    pub(super) async fn start(&self) -> Result<(), ProviderError> {
        self.docker_client
            .container_exec(
                &self.container_name,
                vec!["sh", "-c", "echo start > /tmp/zombiepipe"],
                None,
                None,
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to start pod {} after spawning", self.name),
                    err.into(),
                )
            })?
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to start pod {} after spawning", self.name,),
                    anyhow!("command failed in container: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    fn get_remote_parent_dir(&self, remote_file_path: &Path) -> Option<PathBuf> {
        if let Some(remote_parent_dir) = remote_file_path.parent() {
            if matches!(
                remote_parent_dir.components().rev().peekable().peek(),
                Some(Component::Normal(_))
            ) {
                return Some(remote_parent_dir.to_path_buf());
            }
        }

        None
    }

    async fn create_remote_dir(&self, remote_dir: &Path) -> Result<(), ProviderError> {
        let _ = self
            .docker_client
            .container_exec(
                &self.container_name,
                vec!["mkdir", "-p", &remote_dir.to_string_lossy()],
                None,
                Some("root"),
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!(
                        "failed to create dir {} for container {}",
                        remote_dir.to_string_lossy(),
                        &self.name
                    ),
                    err.into(),
                )
            })?;

        Ok(())
    }

    fn namespace_name(&self) -> String {
        self.namespace
            .upgrade()
            .map(|namespace| namespace.name().to_string())
            .unwrap_or_else(|| panic!("namespace shouldn't be dropped, {THIS_IS_A_BUG}"))
    }
}

#[async_trait]
impl<FS> ProviderNode for DockerNode<FS>
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

    fn log_cmd(&self) -> String {
        format!(
            "{} logs -f {}",
            self.docker_client.client_binary(),
            self.container_name
        )
    }

    fn path_in_node(&self, file: &Path) -> PathBuf {
        // here is just a noop op since we will receive the path
        // for the file inside the pod
        PathBuf::from(file)
    }

    async fn logs(&self) -> Result<String, ProviderError> {
        self.docker_client
            .container_logs(&self.container_name)
            .await
            .map_err(|err| ProviderError::GetLogsFailed(self.name.to_string(), err.into()))
    }

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError> {
        let logs = self.logs().await?;

        self.filesystem
            .write(local_dest, logs)
            .await
            .map_err(|err| ProviderError::DumpLogsFailed(self.name.to_string(), err.into()))?;

        Ok(())
    }

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        debug!(
            "running command for {} with options {:?}",
            self.name, options
        );
        let command = [vec![options.program], options.args].concat();

        self.docker_client
            .container_exec(
                &self.container_name,
                vec!["sh", "-c", &command.join(" ")],
                Some(
                    options
                        .env
                        .iter()
                        .map(|(k, v)| (k.as_ref(), v.as_ref()))
                        .collect(),
                ),
                None,
            )
            .await
            .map_err(|err| {
                ProviderError::RunCommandError(
                    format!("sh -c {}", &command.join(" ")),
                    format!("in pod {}", self.name),
                    err.into(),
                )
            })
    }

    async fn run_script(
        &self,
        options: RunScriptOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let file_name = options
            .local_script_path
            .file_name()
            .expect(&format!(
                "file name should be present at this point {THIS_IS_A_BUG}"
            ))
            .to_string_lossy();

        let remote_script_path = self.scripts_dir.join(Path::new(file_name.as_ref()));

        // Upload the script to the container and make it executable
        self.send_file(&options.local_script_path, &remote_script_path, "0755")
            .await?;

        // Run the script with provided arguments and environment
        self.run_command(RunCommandOptions {
            program: remote_script_path.to_string_lossy().to_string(),
            args: options.args,
            env: options.env,
        })
        .await
        .map_err(|err| ProviderError::RunScriptError(self.name.to_string(), err.into()))
    }

    async fn send_file(
        &self,
        local_file_path: &Path,
        remote_file_path: &Path,
        mode: &str,
    ) -> Result<(), ProviderError> {
        if let Some(remote_parent_dir) = self.get_remote_parent_dir(remote_file_path) {
            self.create_remote_dir(&remote_parent_dir).await?;
        }

        debug!(
            "starting sending file for {}: {} to {} with mode {}",
            self.name,
            local_file_path.to_string_lossy(),
            remote_file_path.to_string_lossy(),
            mode
        );

        self.docker_client
            .container_cp(&self.container_name, local_file_path, remote_file_path)
            .await
            .map_err(|err| {
                ProviderError::SendFile(
                    local_file_path.to_string_lossy().to_string(),
                    self.name.clone(),
                    err.into(),
                )
            })?;

        self.docker_client
            .container_exec(
                &self.container_name,
                vec!["chmod", mode, &remote_file_path.to_string_lossy()],
                None,
                Some("root"),
            )
            .await
            .map_err(|err| {
                ProviderError::SendFile(
                    self.name.clone(),
                    local_file_path.to_string_lossy().to_string(),
                    err.into(),
                )
            })?
            .map_err(|err| {
                ProviderError::SendFile(
                    self.name.clone(),
                    local_file_path.to_string_lossy().to_string(),
                    anyhow!("chmod failed: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn receive_file(
        &self,
        _remote_src: &Path,
        _local_dest: &Path,
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn ip(&self) -> Result<IpAddr, ProviderError> {
        let ip = self
            .docker_client
            .container_ip(&self.container_name)
            .await
            .map_err(|err| {
                ProviderError::InvalidConfig(format!("Error getting container ip, err: {err}"))
            })?;

        Ok(ip.parse::<IpAddr>().map_err(|err| {
            ProviderError::InvalidConfig(format!(
                "Can not parse the container ip: {ip}, err: {err}"
            ))
        })?)
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        self.docker_client
            .container_exec(
                &self.container_name,
                vec!["sh", "-c", "echo pause > /tmp/zombiepipe"],
                None,
                None,
            )
            .await
            .map_err(|err| ProviderError::PauseNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::PauseNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when pausing node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        self.docker_client
            .container_exec(
                &self.container_name,
                vec!["sh", "-c", "echo resume > /tmp/zombiepipe"],
                None,
                None,
            )
            .await
            .map_err(|err| ProviderError::PauseNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::PauseNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when pausing node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError> {
        if let Some(duration) = after {
            sleep(duration).await;
        }

        self.docker_client
            .container_exec(
                &self.container_name,
                vec!["sh", "-c", "echo restart > /tmp/zombiepipe"],
                None,
                None,
            )
            .await
            .map_err(|err| ProviderError::PauseNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::PauseNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when pausing node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        self.docker_client
            .container_rm(&self.container_name)
            .await
            .map_err(|err| ProviderError::KillNodeFailed(self.name.to_string(), err.into()))?;

        if let Some(namespace) = self.namespace.upgrade() {
            namespace.nodes.write().await.remove(&self.name);
        }

        Ok(())
    }
}
