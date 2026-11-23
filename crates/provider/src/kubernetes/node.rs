use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    net::IpAddr,
    path::{Component, Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::{
    shared::{
        resources::Resources,
        volume::{NodeVolume, VolumeDataSource, VolumeSource},
    },
    types::AssetLocation,
};
use futures::future::try_join_all;
use k8s_openapi::api::core::v1::{ContainerStatus, PodStatus, ServicePort, ServiceSpec};
use serde::{Deserialize, Serialize};
use support::{constants::THIS_IS_A_BUG, fs::FileSystem};
use tokio::{sync::RwLock, task::JoinHandle, time::sleep, try_join};
use tracing::{debug, trace, warn};
use url::Url;

use super::{
    client::{
        helpers::{status_is_done, status_is_ready},
        KubernetesClient, Lookup, SecretLookup,
    },
    namespace::KubernetesNamespace,
    pod_spec_builder::{FileToFetch, PodSpecBuilder},
};
use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, NODE_SCRIPTS_DIR},
    kubernetes,
    shared::helpers::{default_true, ensure_wrapper},
    types::{
        DbSnapshot, ExecutionResult, InnerSnapshotDb, NodeRole, Port, ProcessStatus,
        ReadinessProbe, RunCommandOptions, RunScriptOptions, SecretEnvVar, TransferedFile,
    },
    ProviderError, ProviderNamespace, ProviderNode,
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
    /// Env read from `Secret`s in the namespace, by the pod.
    pub(super) secret_env: &'a [SecretEnvVar],
    pub(super) startup_files: &'a [TransferedFile],
    pub(super) resources: Option<&'a Resources>,
    pub(super) db_snapshot: Option<&'a DbSnapshot>,
    pub(super) role: Option<NodeRole>,
    /// Named ports exposed on the pod's `Service`, headless when none.
    pub(super) ports: &'a [(String, Port)],
    /// Whether `program` runs through the zombie wrapper (see
    /// [`SpawnNodeOptions::wrapper`](crate::types::SpawnNodeOptions)).
    pub(super) wrapper: bool,
    /// The pod's readiness probe, for a process without the wrapper.
    pub(super) readiness: Option<&'a ReadinessProbe>,
    /// Volumes backed by a `PersistentVolumeClaim`.
    pub(super) volumes: &'a [NodeVolume],
    pub(super) k8s_client: &'a KubernetesClient,
    pub(super) filesystem: &'a FS,
}

impl<'a, FS> KubernetesNodeOptions<'a, FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) fn from_deserializable(
        deserializable: &'a DeserializableKubernetesNodeOptions,
        namespace: &'a Weak<KubernetesNamespace<FS>>,
        namespace_base_dir: &'a PathBuf,
        k8s_client: &'a KubernetesClient,
        filesystem: &'a FS,
    ) -> KubernetesNodeOptions<'a, FS> {
        KubernetesNodeOptions {
            namespace,
            namespace_base_dir,
            name: &deserializable.name,
            image: deserializable.image.as_ref(),
            program: &deserializable.program,
            args: &deserializable.args,
            env: &deserializable.env,
            secret_env: &[],
            startup_files: &[],
            resources: deserializable.resources.as_ref(),
            db_snapshot: None,
            role: None,
            ports: &deserializable.ports,
            wrapper: deserializable.wrapper,
            readiness: None,
            volumes: &deserializable.volumes,
            k8s_client,
            filesystem,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct DeserializableKubernetesNodeOptions {
    pub(super) name: String,
    pub(super) image: Option<String>,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) env: Vec<(String, String)>,
    pub(super) resources: Option<Resources>,
    #[serde(default)]
    pub(super) ports: Vec<(String, Port)>,
    #[serde(default = "default_true")]
    pub(super) wrapper: bool,
    #[serde(default)]
    pub(super) volumes: Vec<NodeVolume>,
}

type FwdInfo = (u16, JoinHandle<()>);

#[derive(Serialize)]
pub(super) struct KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    #[serde(skip)]
    namespace: Weak<KubernetesNamespace<FS>>,
    name: String,
    image: String,
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    resources: Option<Resources>,
    /// Named ports exposed on the pod's `Service`, headless when none.
    ports: Vec<(String, Port)>,
    /// Whether `program` runs through the zombie wrapper.
    wrapper: bool,
    /// Volumes backed by a `PersistentVolumeClaim`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    volumes: Vec<NodeVolume>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    relay_data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    #[serde(skip)]
    k8s_client: KubernetesClient,
    #[serde(skip)]
    filesystem: FS,
    #[serde(skip)]
    port_fwds: RwLock<HashMap<u16, FwdInfo>>,
    provider_tag: String,
}

impl<FS> KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        options: KubernetesNodeOptions<'_, FS>,
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

        let node = Arc::new(KubernetesNode {
            namespace: options.namespace.clone(),
            name: options.name.to_string(),
            image: image.to_string(),
            program: options.program.to_string(),
            args: options.args.to_vec(),
            env: options.env.to_vec(),
            resources: options.resources.cloned(),
            ports: options.ports.to_vec(),
            wrapper: options.wrapper,
            volumes: options.volumes.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            k8s_client: options.k8s_client.clone(),
            port_fwds: Default::default(),
            provider_tag: kubernetes::provider::PROVIDER_NAME.to_string(),
        });

        // A direct process runs `program` from the moment the pod is up, so
        // its files are fetched by an init container, in place before that; a
        // wrapped node gets them sent while the wrapper holds `program` back.
        let mounted_files: &[TransferedFile] = if node.wrapper {
            &[]
        } else {
            options.startup_files
        };
        node.initialize_k8s(
            options.role,
            options.readiness,
            options.secret_env,
            mounted_files,
        )
        .await?;

        if let Some(db_snap) = options.db_snapshot {
            node.initialize_db_snapshot(db_snap).await?;
        }

        if node.wrapper {
            node.initialize_startup_files(options.startup_files).await?;
            node.start().await?;
        }

        Ok(node)
    }

    pub(super) async fn attach_to_live(
        options: KubernetesNodeOptions<'_, FS>,
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

        let node = Arc::new(KubernetesNode {
            namespace: options.namespace.clone(),
            name: options.name.to_string(),
            image: image.to_string(),
            program: options.program.to_string(),
            args: options.args.to_vec(),
            env: options.env.to_vec(),
            resources: options.resources.cloned(),
            ports: options.ports.to_vec(),
            wrapper: options.wrapper,
            volumes: options.volumes.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            k8s_client: options.k8s_client.clone(),
            port_fwds: Default::default(),
            provider_tag: kubernetes::provider::PROVIDER_NAME.to_string(),
        });

        Ok(node)
    }

    async fn initialize_k8s(
        &self,
        role: Option<NodeRole>,
        readiness: Option<&ReadinessProbe>,
        secret_env: &[SecretEnvVar],
        files: &[TransferedFile],
    ) -> Result<(), ProviderError> {
        let mut labels = BTreeMap::from([
            (
                "app.kubernetes.io/name".to_string(),
                self.name().to_string().to_lowercase(),
            ),
            (
                "x-infra-instance".to_string(),
                env::var("X_INFRA_INSTANCE")
                    .unwrap_or("ondemand".to_string())
                    .to_lowercase(),
            ),
        ]);
        if let Some(role) = role {
            labels.insert("zombie-role".to_string(), role.as_label().to_string());
        }

        let secrets = async {
            check_secrets(&self.k8s_client, &self.namespace_name(), secret_env)
                .await
                .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err))
        };
        let claims = async {
            check_claims(&self.k8s_client, &self.namespace_name(), &self.volumes)
                .await
                .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err))
        };
        let ((), (), files) = try_join!(secrets, claims, self.upload_injected_files(files))?;

        // Create pod
        let pod_spec = PodSpecBuilder::build(
            &self.name,
            &self.image,
            self.resources.as_ref(),
            &self.program,
            &self.args,
            &self.env,
            secret_env,
            &files,
            &self.ports,
            readiness,
            &self.volumes,
            self.wrapper,
        );

        let manifest = self
            .k8s_client
            .create_pod(
                &self.namespace_name(),
                &self.name,
                pod_spec,
                labels.clone(),
                self.wrapper,
            )
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err.into()))?;

        let serialized_manifest = serde_yaml::to_string(&manifest)
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;

        let dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from(format!("{}_manifest.yaml", self.name)),
        ]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;

        // Create service for pod, with the ports the spawn declared; headless
        // when it declared none (a ClusterIP Service needs at least one port,
        // a headless one does not).
        let service_ports: Vec<ServicePort> = self
            .ports
            .iter()
            .map(|(name, port)| ServicePort {
                port: (*port).into(),
                name: Some(name.clone()),
                ..Default::default()
            })
            .collect();
        let service_spec = ServiceSpec {
            selector: Some(labels.clone()),
            cluster_ip: service_ports.is_empty().then(|| "None".to_string()),
            ports: (!service_ports.is_empty()).then_some(service_ports),
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
            &PathBuf::from(format!("{}_svc_manifest.yaml", self.name)),
        ]);

        self.filesystem
            .write(service_dest_path, serialized_service_manifest)
            .await?;

        Ok(())
    }

    /// Upload a direct process's injected files to the network's fileserver,
    /// where the pod's init container fetches them from.
    async fn upload_injected_files(
        &self,
        files: &[TransferedFile],
    ) -> Result<Vec<FileToFetch>, ProviderError> {
        try_join_all(files.iter().map(|file| async move {
            let (url, hash) = self.upload_to_fileserver(&file.local_path).await?;
            Ok::<_, ProviderError>(FileToFetch {
                file: file.clone(),
                url: make_url_internal(&url).to_string(),
                hash,
            })
        }))
        .await
        .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err.into()))
    }

    async fn initialize_db_snapshot(&self, db_snapshot: &DbSnapshot) -> Result<(), ProviderError> {
        trace!("snap: {db_snapshot}");
        let url_of_snap = match db_snapshot {
            // The orchestrator already downloaded/copied the snapshot into
            // the namespace cache; we still need the pod to fetch it over
            // HTTP, so upload to the fileserver and curl from inside.
            DbSnapshot::Local(path) => self.upload_to_fileserver(path).await?.0,
            // The fileserver fetched it itself (see
            // `ProviderNamespace::fetch_into_fileserver`): just curl it.
            DbSnapshot::InFileserver(url) => url.clone(),
        };
        let url_of_snap = make_url_internal(&url_of_snap);

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
            // Use our version of curl, without progress meter (but still
            // reporting errors) and failing on HTTP errors.
            "/cfg/curl",
            "-sS",
            "--fail",
            url_of_snap.as_ref(),
            "--output",
            "/data/db.tgz",
            "&&",
            "cd",
            "/",
            "&&",
            "tar",
            "--skip-old-files",
            "-xzf",
            "/data/db.tgz",
            "&&",
            "rm",
            "-f",
            "/data/db.tgz",
        ]);

        trace!("cmd opts: {:#?}", opts);
        self.run_command(opts).await?.map_err(|(status, stderr)| {
            ProviderError::NodeSpawningFailed(
                format!("failed to initialize db snapshot for {}", self.name),
                anyhow!("command failed in container: status {status}: {stderr}"),
            )
        })?;

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
                    format!(
                        "failed to create dir {} for pod {}",
                        remote_dir.to_string_lossy(),
                        self.name
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

    async fn upload_to_fileserver(&self, location: &Path) -> Result<(Url, String), ProviderError> {
        if let Some(namespace) = self.namespace.upgrade() {
            namespace.upload_file(location).await
        } else {
            Err(ProviderError::FailedToUpgradeWeakRef)
        }
    }

    async fn download_file(
        &self,
        url: &str,
        remote_file_path: &Path,
        hash: Option<&str>,
    ) -> Result<(), ProviderError> {
        let r = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec![
                    "/cfg/curl",
                    url,
                    "--output",
                    &remote_file_path.to_string_lossy(),
                ],
            )
            .await
            .map_err(|err| {
                ProviderError::DownloadFile(
                    remote_file_path.to_string_lossy().to_string(),
                    anyhow!(format!("node: {}, err: {}", self.name(), err)),
                )
            })?;

        trace!("download url {} result: {:?}", url, r);

        if r.is_err() {
            return Err(ProviderError::DownloadFile(
                remote_file_path.to_string_lossy().to_string(),
                anyhow!(format!("node: {}, err downloading file", self.name())),
            ));
        }

        if let Some(hash) = hash {
            // check if the hash of the file is correct
            let res = self
                .k8s_client
                .pod_exec(
                    &self.namespace_name(),
                    &self.name,
                    vec![
                        "/cfg/coreutils",
                        "sha256sum",
                        &remote_file_path.to_string_lossy(),
                    ],
                )
                .await
                .map_err(|err| {
                    ProviderError::DownloadFile(
                        remote_file_path.to_string_lossy().to_string(),
                        anyhow!(format!("node: {}, err: {}", self.name(), err)),
                    )
                })?;

            if let Ok(output) = res {
                if !output.contains(hash) {
                    return Err(ProviderError::DownloadFile(
                        remote_file_path.to_string_lossy().to_string(),
                        anyhow!(format!("node: {}, invalid sha256sum hash: {hash} for file, output was {output}", self.name())),
                    ));
                }
            } else {
                return Err(ProviderError::DownloadFile(
                    remote_file_path.to_string_lossy().to_string(),
                    anyhow!(format!(
                        "node: {}, err calculating sha256sum for file {:?}",
                        self.name(),
                        res
                    )),
                ));
            }
        }

        Ok(())
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

    fn log_cmd(&self) -> String {
        format!("kubectl -n {} logs {}", self.namespace_name(), self.name)
    }

    fn path_in_node(&self, file: &Path) -> PathBuf {
        // here is just a noop op since we will receive the path
        // for the file inside the pod
        PathBuf::from(file)
    }

    // TODO: handle log rotation as we do in v1
    async fn logs(&self) -> Result<String, ProviderError> {
        let namespace = self.namespace_name();
        let failed = |err: anyhow::Error| ProviderError::GetLogsFailed(self.name.to_string(), err);
        match self.k8s_client.pod_logs(&namespace, &self.name, None).await {
            Ok(logs) => Ok(logs),
            // the main container never ran: what the init container that
            // failed, and so ended the pod, said
            Err(err) => {
                let status = self
                    .k8s_client
                    .pod_status(&namespace, &self.name)
                    .await
                    .map_err(|err| failed(err.into()))?;
                let Some(init) = failed_init_container(&status) else {
                    return Err(failed(err.into()));
                };
                self.k8s_client
                    .pod_logs(&namespace, &self.name, Some(&init.name))
                    .await
                    .map_err(|err| failed(err.into()))
            },
        }
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
                    format!("sh -c {}", command.join(" ")),
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
        if let Some(remote_parent_dir) = self.get_remote_parent_dir(remote_file_path) {
            self.create_remote_dir(&remote_parent_dir).await?;
        }

        debug!(
            "Uploading file: {} IFF not present in the fileserver",
            local_file_path.to_string_lossy()
        );

        // we need to override the url to use inside the pod
        let (url, hash) = self.upload_to_fileserver(local_file_path).await?;
        let url = make_url_internal(&url);
        // let _ = url.set_host(Some("fileserver"));
        // let _ = url.set_port(Some(80));

        // Sometimes downloading the file fails (the file is corrupted)
        // Add at most 5 retries
        let mut last_err = None;
        for i in 0..5 {
            if i > 0 {
                warn!("retrying number {i} download file {:?}", remote_file_path);
                tokio::time::sleep(Duration::from_secs(i)).await;
            }

            let res = self
                .download_file(url.as_ref(), remote_file_path, Some(&hash))
                .await;

            last_err = res.err();

            if last_err.is_none() {
                // ready to continue
                break;
            }
        }

        if let Some(last_err) = last_err {
            return Err(last_err);
        }

        let _ = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["chmod", mode, &remote_file_path.to_string_lossy()],
            )
            .await
            .map_err(|err| {
                ProviderError::SendFile(
                    self.name.clone(),
                    local_file_path.to_string_lossy().to_string(),
                    err.into(),
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

    /// The pod's phase and, while it runs, its Ready condition. A direct
    /// process's pod ends with it (see `PodSpecBuilder`), so an exit shows as
    /// a terminal phase with the container's exit code.
    async fn status(&self) -> Result<ProcessStatus, ProviderError> {
        let status = self
            .k8s_client
            .pod_status(&self.namespace_name(), &self.name)
            .await
            .map_err(|err| {
                ProviderError::InvalidConfig(format!(
                    "{}: could not read the pod: {err}",
                    self.name
                ))
            })?;

        if status_is_done(&status) {
            // the container's code, or the init container's that ended the pod
            let code = status
                .container_statuses
                .iter()
                .flatten()
                .find(|c| c.name == self.name)
                .and_then(exit_code)
                .or_else(|| failed_init_container(&status).and_then(exit_code));
            return Ok(ProcessStatus::Exited { code });
        }
        Ok(ProcessStatus::Running {
            ready: Some(status_is_ready(&status)),
        })
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
                ProviderError::InvalidConfig(format!("Can not parse the pod ip: {ip}, err: {err}"))
            })?)
        } else {
            Err(ProviderError::InvalidConfig(format!(
                "Can not find ip of pod: {}",
                self.name()
            )))
        }
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        ensure_wrapper(
            &self.name,
            self.wrapper,
            "pause",
            ProviderError::PauseNodeFailed,
        )?;

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", "echo pause > /tmp/zombiepipe"],
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
        ensure_wrapper(
            &self.name,
            self.wrapper,
            "resume",
            ProviderError::ResumeNodeFailed,
        )?;

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", "echo resume > /tmp/zombiepipe"],
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
        ensure_wrapper(
            &self.name,
            self.wrapper,
            "restart",
            ProviderError::RestartNodeFailed,
        )?;

        if let Some(duration) = after {
            sleep(duration).await;
        }

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", "echo restart > /tmp/zombiepipe"],
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

    async fn restart_with(
        &self,
        assets: &[AssetLocation],
        cmd: &str,
        args: &[String],
        after: Option<Duration>,
    ) -> Result<(), ProviderError> {
        ensure_wrapper(
            &self.name,
            self.wrapper,
            "restart",
            ProviderError::RestartNodeFailed,
        )?;

        // get the assets
        for asset in assets {
            match asset {
                AssetLocation::Url(url) => {
                    let url = url.to_string();
                    trace!("downloading asset into node {} from {}", self.name, url);

                    let asset_remote_filepath =
                        format!("{NODE_SCRIPTS_DIR}/{}", asset.extract_name());
                    let args = &[
                        url.as_str(),
                        "-L",
                        "-O",
                        "--output-dir",
                        // use the script dir from the container
                        NODE_SCRIPTS_DIR,
                        "&&",
                        "chmod",
                        "777",
                        asset_remote_filepath.as_str(),
                    ];

                    let opts = RunCommandOptions::new("/cfg/curl").args(args);
                    let _ = self.run_command(opts).await?;
                },
                AssetLocation::FilePath(path) => {
                    trace!(
                        "uploading local asset {} into container {}",
                        path.to_string_lossy(),
                        self.name()
                    );

                    let fullpath = format!("{NODE_SCRIPTS_DIR}/{}", asset.extract_name());

                    self.send_file(path.as_path(), Path::new(&fullpath), "0777")
                        .await?;
                },
            }
        }

        // update zombie.cmd file
        let scoped_cmd = format!("{NODE_SCRIPTS_DIR}/{}", cmd);
        let update_cmd = format!(
            "echo 'update-cmd {} {}' > /tmp/zombiepipe",
            scoped_cmd,
            args.join(" ")
        );

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", update_cmd.as_str()],
            )
            .await
            .map_err(|err| ProviderError::RestartNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::RestartNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when restarting node: status {}: {}", err.0, err.1),
                )
            })?;

        tracing::debug!(
            "🔄 [{}], restarting with command: {} {}",
            self.name(),
            &scoped_cmd,
            &args.join(" ")
        );
        // now restart
        self.restart(after).await
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

    async fn snapshot_db(&self, _is_cumulus_based: bool) -> Result<InnerSnapshotDb, ProviderError> {
        todo!("snapshot db is not implemented yet for k8s");
    }
}

/// The exit code of a container that terminated.
fn exit_code(container: &ContainerStatus) -> Option<i32> {
    container
        .state
        .as_ref()?
        .terminated
        .as_ref()
        .map(|t| t.exit_code)
}

/// The init container that failed, which is what ended the pod before its
/// own container ran.
fn failed_init_container(status: &PodStatus) -> Option<&ContainerStatus> {
    status
        .init_container_statuses
        .iter()
        .flatten()
        .find(|c| exit_code(c).is_some_and(|code| code != 0))
}

// Helper fn to make a fileserver usable from inside of pods
fn make_url_internal(url: &Url) -> Url {
    let mut url = url.clone();
    let _ = url.set_host(Some("fileserver"));
    let _ = url.set_port(Some(80));
    url
}

/// A Secret or key that is not there would hold the pod in
/// `CreateContainerConfigError` until the spawn times out, so it is looked
/// up first. A Secret this identity may not read is left to the pod, which
/// needs no such permission.
async fn check_secrets(
    client: &KubernetesClient,
    namespace: &str,
    secret_env: &[SecretEnvVar],
) -> Result<(), anyhow::Error> {
    // each Secret once, all at the same time
    let names: Vec<&str> = secret_env
        .iter()
        .map(|var| var.secret.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let lookups = try_join_all(names.iter().map(|name| client.get_secret(namespace, name))).await?;
    let secrets: BTreeMap<&str, SecretLookup> = names.into_iter().zip(lookups).collect();
    for var in secret_env {
        let secret = match &secrets[var.secret.as_str()] {
            SecretLookup::Found(secret) => secret,
            SecretLookup::NotFound => {
                return Err(anyhow!(
                    "env {}: secret '{}' not found in namespace {namespace}",
                    var.name,
                    var.secret
                ))
            },
            SecretLookup::Forbidden => {
                warn!(
                    "secret '{}' in namespace {namespace} may not be read, env {} is left to the pod",
                    var.secret, var.name
                );
                continue;
            },
        };
        if !secret
            .data
            .as_ref()
            .is_some_and(|data| data.contains_key(&var.key))
        {
            return Err(anyhow!(
                "env {}: secret '{}' has no key '{}'",
                var.name,
                var.secret,
                var.key
            ));
        }
    }
    Ok(())
}

/// An existing claim that is not there, or lost, would hold the pod in
/// `Pending` until the spawn times out, so the claims a node mounts (or a
/// provisioned one is cloned from) are looked up first. A claim this identity
/// may not read is left to the pod.
async fn check_claims(
    client: &KubernetesClient,
    namespace: &str,
    volumes: &[NodeVolume],
) -> Result<(), anyhow::Error> {
    // (claim, mounted read-write, what it is for)
    let wanted: Vec<(&str, bool, String)> = volumes
        .iter()
        .filter_map(|volume| {
            let path = volume.mount_path().display();
            match volume.source() {
                VolumeSource::Claim { name } => Some((
                    name.as_str(),
                    !volume.read_only(),
                    format!("volume '{path}': claim '{name}'"),
                )),
                VolumeSource::Template {
                    from: Some(VolumeDataSource::Claim(name)),
                    ..
                } => Some((
                    name.as_str(),
                    false,
                    format!("volume '{path}': from_claim '{name}'"),
                )),
                VolumeSource::Template { .. } => None,
            }
        })
        .collect();
    let lookups = try_join_all(
        wanted
            .iter()
            .map(|(name, ..)| client.get_pvc(namespace, name)),
    )
    .await?;
    for ((_, read_write, what), lookup) in wanted.iter().zip(lookups) {
        let pvc = match lookup {
            Lookup::Found(pvc) => pvc,
            Lookup::NotFound => {
                return Err(anyhow!(
                    "{what} not found in namespace {namespace}; a claim has to exist before \
                     the network spawns, in the namespace it spawns in (an existing one)"
                ))
            },
            Lookup::Forbidden => {
                warn!("{what} in namespace {namespace} may not be read, it is left to the pod");
                continue;
            },
        };
        if pvc.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("Lost") {
            return Err(anyhow!("{what} has lost its volume"));
        }
        let modes: Vec<&str> = pvc
            .spec
            .as_ref()
            .and_then(|spec| spec.access_modes.as_ref())
            .map(|modes| modes.iter().map(String::as_str).collect())
            .unwrap_or_default();
        if *read_write && modes.contains(&"ReadWriteMany") {
            return Err(anyhow!(
                "{what} is ReadWriteMany; mount it read_only, a volume has a single writer"
            ));
        }
        if *read_write && !modes.is_empty() && modes.iter().all(|m| *m == "ReadOnlyMany") {
            return Err(anyhow!("{what} is ReadOnlyMany; mount it read_only"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mockito::{Matcher, Server, ServerGuard};

    use super::*;

    fn client(server: &ServerGuard) -> KubernetesClient {
        let config = kube::Config::new(server.url().parse().unwrap());
        KubernetesClient::from_inner(kube::Client::try_from(config).unwrap())
    }

    fn turn_secret() -> Vec<SecretEnvVar> {
        vec![SecretEnvVar {
            name: "TURN_SECRET".into(),
            secret: "turn".into(),
            key: "secret".into(),
        }]
    }

    async fn mock_secret(server: &mut ServerGuard, status: usize, body: &str) {
        server
            .mock("GET", "/api/v1/namespaces/ns/secrets/turn")
            .match_query(Matcher::Any)
            .with_status(status)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;
    }

    fn claim_volume(path: &str, claim: &str, read_only: bool) -> NodeVolume {
        let builder = configuration::shared::volume::NodeVolumeBuilder::new()
            .with_mount_path(path)
            .with_claim(claim);
        let builder = if read_only {
            builder.with_read_only()
        } else {
            builder
        };
        builder.build().unwrap()
    }

    async fn mock_pvc(server: &mut ServerGuard, status: usize, body: &str) {
        server
            .mock("GET", "/api/v1/namespaces/ns/persistentvolumeclaims/db")
            .match_query(Matcher::Any)
            .with_status(status)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;
    }

    fn pvc_body(modes: &str, phase: &str) -> String {
        format!(
            r#"{{"apiVersion":"v1","kind":"PersistentVolumeClaim","metadata":{{"name":"db"}},"spec":{{"accessModes":[{modes}]}},"status":{{"phase":"{phase}"}}}}"#
        )
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_bound_claim_passes() {
        let mut server = Server::new_async().await;
        mock_pvc(
            &mut server,
            200,
            &pvc_body(r#""ReadWriteOncePod""#, "Bound"),
        )
        .await;
        check_claims(
            &client(&server),
            "ns",
            &[claim_volume("/data", "db", false)],
        )
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_missing_claim_fails_naming_it() {
        let mut server = Server::new_async().await;
        mock_pvc(
            &mut server,
            404,
            r#"{"kind":"Status","apiVersion":"v1","status":"Failure","message":"not found","reason":"NotFound","code":404}"#,
        )
        .await;
        let err = check_claims(
            &client(&server),
            "ns",
            &[claim_volume("/data", "db", false)],
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            err.starts_with("volume '/data': claim 'db' not found in namespace ns"),
            "{err}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_lost_claim_fails() {
        let mut server = Server::new_async().await;
        mock_pvc(&mut server, 200, &pvc_body(r#""ReadWriteOnce""#, "Lost")).await;
        let err = check_claims(
            &client(&server),
            "ns",
            &[claim_volume("/data", "db", false)],
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "volume '/data': claim 'db' has lost its volume"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_read_write_many_claim_is_only_mounted_read_only() {
        let mut server = Server::new_async().await;
        mock_pvc(&mut server, 200, &pvc_body(r#""ReadWriteMany""#, "Bound")).await;
        let err = check_claims(
            &client(&server),
            "ns",
            &[claim_volume("/data", "db", false)],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("is ReadWriteMany"), "{err}");
        check_claims(&client(&server), "ns", &[claim_volume("/data", "db", true)])
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_read_only_many_claim_is_not_mounted_read_write() {
        let mut server = Server::new_async().await;
        mock_pvc(&mut server, 200, &pvc_body(r#""ReadOnlyMany""#, "Bound")).await;
        let err = check_claims(
            &client(&server),
            "ns",
            &[claim_volume("/data", "db", false)],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("is ReadOnlyMany"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn provisioned_volumes_need_no_lookup() {
        // no mock: any request would fail
        let server = Server::new_async().await;
        let template = configuration::shared::volume::NodeVolumeBuilder::new()
            .with_mount_path("/data")
            .build()
            .unwrap();
        check_claims(&client(&server), "ns", &[template])
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_secret_with_the_key_passes() {
        let mut server = Server::new_async().await;
        mock_secret(
            &mut server,
            200,
            r#"{"apiVersion":"v1","kind":"Secret","metadata":{"name":"turn"},"data":{"secret":"aGk="}}"#,
        )
        .await;
        check_secrets(&client(&server), "ns", &turn_secret())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_secret_without_the_key_fails_naming_it() {
        let mut server = Server::new_async().await;
        mock_secret(
            &mut server,
            200,
            r#"{"apiVersion":"v1","kind":"Secret","metadata":{"name":"turn"},"data":{"other":"aGk="}}"#,
        )
        .await;
        let err = check_secrets(&client(&server), "ns", &turn_secret())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "env TURN_SECRET: secret 'turn' has no key 'secret'"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_missing_secret_fails_naming_it() {
        let mut server = Server::new_async().await;
        mock_secret(
            &mut server,
            404,
            r#"{"kind":"Status","apiVersion":"v1","status":"Failure","message":"not found","reason":"NotFound","code":404}"#,
        )
        .await;
        let err = check_secrets(&client(&server), "ns", &turn_secret())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "env TURN_SECRET: secret 'turn' not found in namespace ns"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn not_being_allowed_to_read_secrets_skips_the_check() {
        let mut server = Server::new_async().await;
        mock_secret(
            &mut server,
            403,
            r#"{"kind":"Status","apiVersion":"v1","status":"Failure","message":"forbidden","reason":"Forbidden","code":403}"#,
        )
        .await;
        check_secrets(&client(&server), "ns", &turn_secret())
            .await
            .unwrap();
    }
}
