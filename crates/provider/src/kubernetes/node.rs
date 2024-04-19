use std::{
    collections::{BTreeMap, HashMap},
    env,
    net::IpAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::{
    shared::{constants::THIS_IS_A_BUG, resources::Resources},
    types::AssetLocation,
};
use futures::future::try_join_all;
use k8s_openapi::api::core::v1::{ServicePort, ServiceSpec};
use sha2::Digest;
use support::fs::FileSystem;
use tokio::{sync::RwLock, task::JoinHandle, time::sleep, try_join};
use tracing::trace;
use url::Url;

use super::{namespace::KubernetesNamespace, pod_spec_builder::PodSpecBuilder};
use crate::{
    constants::{
        NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, NODE_SCRIPTS_DIR, P2P_PORT,
        PROMETHEUS_PORT, RPC_HTTP_PORT, RPC_WS_PORT,
    },
    types::{ExecutionResult, RunCommandOptions, RunScriptOptions, TransferedFile},
    KubernetesClient, ProviderError, ProviderNamespace, ProviderNode,
};

pub(super) struct KubernetesNodeOptions<'a, FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) namespace: &'a Weak<KubernetesNamespace<FS>>,
    pub(super) namespace_base_dir: &'a PathBuf,
    pub(super) name: &'a str,
    pub(super) image: Option<&'a String>,
    pub(super) program: &'a str,
    pub(super) args: &'a [String],
    pub(super) env: &'a [(String, String)],
    pub(super) startup_files: &'a [TransferedFile],
    pub(super) resources: Option<&'a Resources>,
    pub(super) db_snapshot: Option<&'a AssetLocation>,
    pub(super) k8s_client: &'a KubernetesClient,
    pub(super) filesystem: &'a FS,
}

type FwdInfo = (u16, JoinHandle<()>);

pub(super) struct KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    namespace: Weak<KubernetesNamespace<FS>>,
    name: String,
    args: Vec<String>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    relay_data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    k8s_client: KubernetesClient,
    http_client: reqwest::Client,
    filesystem: FS,
    port_fwds: RwLock<HashMap<u16, FwdInfo>>,
}

impl<FS> KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        options: KubernetesNodeOptions<'_, FS>,
    ) -> Result<Arc<Self>, ProviderError> {
        let filesystem = options.filesystem.clone();

        let base_dir =
            PathBuf::from_iter([options.namespace_base_dir, &PathBuf::from(options.name)]);
        filesystem.create_dir_all(&base_dir).await?;

        let base_dir_raw = base_dir.to_string_lossy();
        let config_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_DATA_DIR));
        let relay_data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_RELAY_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_SCRIPTS_DIR));
        let log_path = base_dir.join("node.log");

        try_join!(
            filesystem.create_dir(&config_dir),
            filesystem.create_dir(&data_dir),
            filesystem.create_dir(&relay_data_dir),
            filesystem.create_dir(&scripts_dir),
        )?;

        let node = Arc::new(KubernetesNode {
            namespace: options.namespace.clone(),
            name: options.name.to_string(),
            args: options.args.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            k8s_client: options.k8s_client.clone(),
            http_client: reqwest::Client::new(),
            port_fwds: Default::default(),
        });

        node.initialize_k8s(
            options.image,
            options.program,
            options.args,
            options.env,
            options.resources,
        )
        .await?;

        if let Some(db_snap) = options.db_snapshot {
            node.initialize_db_snapshot(db_snap).await?;
        }

        node.initialize_startup_files(options.startup_files).await?;

        node.start().await?;

        Ok(node)
    }

    async fn initialize_k8s(
        &self,
        image: Option<&String>,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        resources: Option<&Resources>,
    ) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([
            (
                "app.kubernetes.io/name".to_string(),
                self.name().to_string(),
            ),
            (
                "x-infra-instance".to_string(),
                env::var("X_INFRA_INSTANCE").unwrap_or("ondemand".to_string()),
            ),
        ]);

        let image = image.ok_or_else(|| {
            ProviderError::MissingNodeInfo(self.name.to_string(), "missing image".to_string())
        })?;

        // Create pod
        let pod_spec = PodSpecBuilder::build(&self.name, image, resources, program, args, env);

        let manifest = self
            .k8s_client
            .create_pod(&self.namespace_name(), &self.name, pod_spec, labels.clone())
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err.into()))?;

        let serialized_manifest = serde_yaml::to_string(&manifest)
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;

        let dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from(format!("{}_manifest.yaml", &self.name)),
        ]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;

        // Create service for pod
        let service_spec = ServiceSpec {
            selector: Some(labels.clone()),
            ports: Some(vec![
                ServicePort {
                    port: P2P_PORT.into(),
                    name: Some("p2p".into()),
                    ..Default::default()
                },
                ServicePort {
                    port: RPC_WS_PORT.into(),
                    name: Some("rpc".into()),
                    ..Default::default()
                },
                ServicePort {
                    port: RPC_HTTP_PORT.into(),
                    name: Some("rpc-http".into()),
                    ..Default::default()
                },
                ServicePort {
                    port: PROMETHEUS_PORT.into(),
                    name: Some("prom".into()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        let service_manifest = self
            .k8s_client
            .create_service(&self.namespace_name(), &self.name, service_spec, labels)
            .await
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        let serialized_service_manifest = serde_yaml::to_string(&service_manifest)
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        let service_dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from(format!("{}_svc_manifest.yaml", &self.name)),
        ]);

        self.filesystem
            .write(service_dest_path, serialized_service_manifest)
            .await?;

        Ok(())
    }

    async fn initialize_db_snapshot(
        &self,
        db_snapshot: &AssetLocation,
    ) -> Result<(), ProviderError> {
        trace!("snap: {db_snapshot}");
        let url_of_snap = match db_snapshot {
            AssetLocation::Url(location) => location.clone(),
            AssetLocation::FilePath(filepath) => self.upload_to_fileserver(filepath).await?,
        };

        // we need to get the snapshot from a public access
        // and extract to /data
        let opts = RunCommandOptions::new("mkdir").args([
            "-p",
            "/data/",
            "&&",
            "mkdir",
            "-p",
            "/relay-data/",
            "&&",
            // Use our version of curl
            "/cfg/curl",
            url_of_snap.as_ref(),
            "--output",
            "/data/db.tgz",
            "&&",
            "cd",
            "/",
            "&&",
            "tar",
            "--skip-old-files",
            "-xzvf",
            "/data/db.tgz",
        ]);

        trace!("cmd opts: {:#?}", opts);
        let _ = self.run_command(opts).await?;

        Ok(())
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
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", "echo start > /tmp/zombiepipe"],
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
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["mkdir", "-p", &remote_dir.to_string_lossy()],
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to created dirfor pod {}", &self.name),
                    err.into(),
                )
            })?;

        Ok(())
    }

    fn namespace_name(&self) -> String {
        self.namespace
            .upgrade()
            .map(|namespace| namespace.name().to_string())
            .unwrap_or_else(|| panic!("namespace shouldn't be dropped, {}", THIS_IS_A_BUG))
    }

    async fn upload_to_fileserver(&self, location: &Path) -> Result<Url, ProviderError> {
        let data = self.filesystem.read(location).await?;
        let hashed_path = hex::encode(sha2::Sha256::digest(&data));
        let req = self
            .http_client
            .head(format!(
                "http://{}/{hashed_path}",
                self.file_server_local_host().await?
            ))
            .build()
            .map_err(|err| {
                ProviderError::UploadFile(location.to_string_lossy().to_string(), err.into())
            })?;

        let url = req.url().clone();
        let res = self.http_client.execute(req).await.map_err(|err| {
            ProviderError::UploadFile(location.to_string_lossy().to_string(), err.into())
        })?;

        if res.status() != reqwest::StatusCode::OK {
            // we need to upload the file
            self.http_client
                .post(url.as_ref())
                .body(data)
                .send()
                .await
                .map_err(|err| {
                    ProviderError::UploadFile(location.to_string_lossy().to_string(), err.into())
                })?;
        }

        Ok(url)
    }

    async fn file_server_local_host(&self) -> Result<String, ProviderError> {
        if let Some(namespace) = self.namespace.upgrade() {
            if let Some(port) = *namespace.file_server_port.read().await {
                return Ok(format!("localhost:{port}"));
            }
        }

        Err(ProviderError::FileServerSetupError(anyhow!(
            "file server port not bound locally"
        )))
    }
}

#[async_trait]
impl<FS> ProviderNode for KubernetesNode<FS>
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
        // here is just a noop op since we will receive the path
        // for the file inside the pod
        PathBuf::from(file)
    }

    // TODO: handle log rotation as we do in v1
    async fn logs(&self) -> Result<String, ProviderError> {
        self.k8s_client
            .pod_logs(&self.namespace_name(), &self.name)
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

    async fn create_port_forward(
        &self,
        local_port: u16,
        remote_port: u16,
    ) -> Result<Option<u16>, ProviderError> {
        // If the fwd exist just return the local port
        if let Some(fwd_info) = self.port_fwds.read().await.get(&remote_port) {
            return Ok(Some(fwd_info.0));
        };

        let (port, task) = self
            .k8s_client
            .create_pod_port_forward(&self.namespace_name(), &self.name, local_port, remote_port)
            .await
            .map_err(|err| ProviderError::PortForwardError(local_port, remote_port, err.into()))?;

        self.port_fwds
            .write()
            .await
            .insert(remote_port, (port, task));

        Ok(Some(port))
    }

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let mut command = vec![];

        for (name, value) in options.env {
            command.push(format!("export {name}={value};"));
        }

        command.push(options.program);

        for arg in options.args {
            command.push(arg);
        }

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", &command.join(" ")],
            )
            .await
            .map_err(|err| {
                ProviderError::RunCommandError(
                    format!("sh -c {}", &command.join(" ")),
                    self.name.to_string(),
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

        self.run_command(RunCommandOptions {
            program: format!("/tmp/{file_name}"),
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
        let data = self.filesystem.read(local_file_path).await.unwrap();

        if let Some(remote_parent_dir) = self.get_remote_parent_dir(remote_file_path) {
            self.create_remote_dir(&remote_parent_dir).await?;
        }

        self.http_client
            .post(format!(
                "http://{}{}",
                self.file_server_local_host().await?,
                remote_file_path.to_string_lossy()
            ))
            .body(data)
            .send()
            .await
            .map_err(|err| {
                ProviderError::SendFile(local_file_path.to_string_lossy().to_string(), err.into())
            })?;

        let _ = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec![
                    "/cfg/curl",
                    &format!("fileserver{}", remote_file_path.to_string_lossy()),
                    "--output",
                    &remote_file_path.to_string_lossy(),
                ],
            )
            .await
            .map_err(|err| {
                ProviderError::SendFile(local_file_path.to_string_lossy().to_string(), err.into())
            })?;

        let _ = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["chmod", mode, &remote_file_path.to_string_lossy()],
            )
            .await
            .map_err(|err| {
                ProviderError::SendFile(local_file_path.to_string_lossy().to_string(), err.into())
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
        let status = self
            .k8s_client
            .pod_status(&self.namespace_name(), &self.name)
            .await
            .map_err(|_| ProviderError::MissingNode(self.name.clone()))?;

        if let Some(ip) = status.pod_ip {
            // Pod ip should be parseable
            Ok(ip.parse::<IpAddr>().map_err(|err| {
                ProviderError::InvalidConfig(format!(
                    "Can not parse the pod ip: {}, err: {}",
                    ip, err
                ))
            })?)
        } else {
            Err(ProviderError::InvalidConfig(format!(
                "Can not find ip of pod: {}",
                self.name()
            )))
        }
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["echo", "pause", ">", "/tmp/zombiepipe"],
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
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["echo", "resume", ">", "/tmp/zombiepipe"],
            )
            .await
            .map_err(|err| ProviderError::ResumeNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::ResumeNodeFailed(
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

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["echo", "restart", ">", "/tmp/zombiepipe"],
            )
            .await
            .map_err(|err| ProviderError::RestartNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::RestartNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when restarting node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        self.k8s_client
            .delete_pod(&self.namespace_name(), &self.name)
            .await
            .map_err(|err| ProviderError::KillNodeFailed(self.name.to_string(), err.into()))?;

        if let Some(namespace) = self.namespace.upgrade() {
            namespace.nodes.write().await.remove(&self.name);
        }

        Ok(())
    }
}
