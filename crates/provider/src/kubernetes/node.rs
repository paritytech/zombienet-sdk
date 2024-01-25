use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::shared::resources::{ResourceQuantity, Resources};
use futures::future::try_join_all;
use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, PodSpec, ResourceRequirements, Volume,
        VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
};
use support::fs::FileSystem;
use tokio::{time::sleep, try_join};

use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, NODE_SCRIPTS_DIR},
    types::{ExecutionResult, RunCommandOptions, RunScriptOptions},
    KubernetesClient, ProviderError, ProviderNode,
};

use super::namespace::KubernetesNamespace;

struct PodSpecBuilder;

impl PodSpecBuilder {
    fn build(
        name: &str,
        image: &str,
        resources: Option<&Resources>,
        program: &str,
        args: &Vec<String>,
        env: &Vec<(String, String)>,
    ) -> PodSpec {
        PodSpec {
            hostname: Some(name.to_string()),
            init_containers: Some(vec![Self::build_helper_binaries_setup_container()]),
            containers: vec![Self::build_main_container(
                name, image, resources, program, args, env,
            )],
            volumes: Some(Self::build_volumes()),
            ..Default::default()
        }
    }

    fn build_main_container(
        name: &str,
        image: &str,
        resources: Option<&Resources>,
        program: &str,
        args: &Vec<String>,
        env: &Vec<(String, String)>,
    ) -> Container {
        Container {
            name: name.to_string(),
            image: Some(image.to_string()),
            image_pull_policy: Some("Always".to_string()),
            command: Some(
                [
                    vec!["/zombie-wrapper.sh".to_string(), program.to_string()],
                    args.clone(),
                ]
                .concat(),
            ),
            env: Some(
                env.iter()
                    .map(|(name, value)| EnvVar {
                        name: name.clone(),
                        value: Some(value.clone()),
                        value_from: None,
                    })
                    .collect(),
            ),
            volume_mounts: Some(Self::build_volume_mounts(vec![VolumeMount {
                name: "zombie-wrapper-volume".to_string(),
                mount_path: "/zombie-wrapper.sh".to_string(),
                sub_path: Some("zombie-wrapper.sh".to_string()),
                ..Default::default()
            }])),
            resources: Self::build_resources_requirements(resources),
            ..Default::default()
        }
    }

    fn build_helper_binaries_setup_container() -> Container {
        Container {
            name: "helper-binaries-setup".to_string(),
            image: Some("docker.io/alpine:latest".to_string()),
            image_pull_policy: Some("Always".to_string()),
            volume_mounts: Some(Self::build_volume_mounts(vec![VolumeMount {
                name: "helper-binaries-downloader-volume".to_string(),
                mount_path: "/helper-binaries-downloader.sh".to_string(),
                sub_path: Some("helper-binaries-downloader.sh".to_string()),
                ..Default::default()
            }])),
            command: Some(vec![
                "ash".to_string(),
                "/helper-binaries-downloader.sh".to_string(),
            ]),
            ..Default::default()
        }
    }

    fn build_volumes() -> Vec<Volume> {
        vec![
            Volume {
                name: "cfg".to_string(),
                ..Default::default()
            },
            Volume {
                name: "data".to_string(),
                ..Default::default()
            },
            Volume {
                name: "relay-data".to_string(),
                ..Default::default()
            },
            Volume {
                name: "zombie-wrapper-volume".to_string(),
                config_map: Some(ConfigMapVolumeSource {
                    name: Some("zombie-wrapper".to_string()),
                    default_mode: Some(0o755),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ]
    }

    fn build_volume_mounts(non_default_mounts: Vec<VolumeMount>) -> Vec<VolumeMount> {
        vec![
            vec![
                VolumeMount {
                    name: "cfg".to_string(),
                    mount_path: "/cfg".to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
                VolumeMount {
                    name: "data".to_string(),
                    mount_path: "/data".to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
                VolumeMount {
                    name: "relay-data".to_string(),
                    mount_path: "/relay-data".to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
            ],
            non_default_mounts,
        ]
        .concat()
    }

    fn build_resources_requirements(resources: Option<&Resources>) -> Option<ResourceRequirements> {
        resources.and_then(|resources| {
            Some(ResourceRequirements {
                limits: Self::build_resources_requirements_quantities(
                    resources.limit_cpu(),
                    resources.limit_memory(),
                ),
                requests: Self::build_resources_requirements_quantities(
                    resources.request_cpu(),
                    resources.request_memory(),
                ),
                ..Default::default()
            })
        })
    }

    fn build_resources_requirements_quantities(
        cpu: Option<&ResourceQuantity>,
        memory: Option<&ResourceQuantity>,
    ) -> Option<BTreeMap<String, Quantity>> {
        let mut quantities = BTreeMap::new();

        if let Some(cpu) = cpu {
            quantities.insert("cpu".to_string(), Quantity(cpu.as_str().to_string()));
        }

        if let Some(memory) = memory {
            quantities.insert("memory".to_string(), Quantity(memory.as_str().to_string()));
        }

        if !quantities.is_empty() {
            Some(quantities)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub(super) struct KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub(super) name: String,
    namespace: Weak<KubernetesNamespace<FS>>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    relay_data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    k8s_client: KubernetesClient,
    http_client: reqwest::Client,
    filesystem: FS,
}

impl<FS> KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        namespace: &Weak<KubernetesNamespace<FS>>,
        name: &str,
        namespace_base_dir: &PathBuf,
        k8s_client: &KubernetesClient,
        filesystem: &FS,
    ) -> Result<Arc<Self>, ProviderError> {
        let base_dir = PathBuf::from_iter([&namespace_base_dir, &PathBuf::from(name)]);
        filesystem.create_dir_all(&base_dir).await?;

        let config_dir = PathBuf::from_iter([&base_dir, &PathBuf::from(NODE_CONFIG_DIR)]);
        let data_dir = PathBuf::from_iter([&base_dir, &PathBuf::from(NODE_DATA_DIR)]);
        let relay_data_dir = PathBuf::from_iter([&base_dir, &PathBuf::from(NODE_RELAY_DATA_DIR)]);
        let scripts_dir = PathBuf::from_iter([&base_dir, &PathBuf::from(NODE_SCRIPTS_DIR)]);
        try_join!(
            filesystem.create_dir(&config_dir),
            filesystem.create_dir(&data_dir),
            filesystem.create_dir(&relay_data_dir),
            filesystem.create_dir(&scripts_dir),
        )?;

        let log_path = PathBuf::from_iter([&base_dir, &PathBuf::from(format!("{name}.log"))]);

        Ok(Arc::new(KubernetesNode {
            namespace: namespace.clone(),
            name: name.to_string(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            k8s_client: k8s_client.clone(),
            http_client: reqwest::Client::new(),
        }))
    }

    pub(super) async fn initialize(
        &self,
        image: Option<&String>,
        resources: Option<&Resources>,
        program: &str,
        args: &Vec<String>,
        env: &Vec<(String, String)>,
    ) -> Result<(), ProviderError> {
        self.initialize_k8s(image, resources, program, args, env)
            .await?;
        self.initialize_startup_files().await?;
        self.start().await?;

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

    async fn initialize_k8s(
        &self,
        image: Option<&String>,
        resources: Option<&Resources>,
        program: &str,
        args: &Vec<String>,
        env: &Vec<(String, String)>,
    ) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([("foo".to_string(), "bar".to_string())]);
        let image = image.ok_or_else(|| {
            ProviderError::MissingNodeInfo(self.name.to_string(), "missing image".to_string())
        })?;

        let pod_spec = PodSpecBuilder::build(&self.name, image, resources, program, args, env);

        let manifest = self
            .k8s_client
            .create_pod(&self.namespace_name(), &self.name, pod_spec, labels)
            .await?;

        let serialized_manifest = serde_yaml::to_string(&manifest)
            .map_err(|err| ProviderError::SomeError(self.name.to_string(), err.into()))?;

        let dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from(format!("{}_manifest.yaml", &self.name)),
        ]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await?;

        Ok(())
    }

    async fn initialize_startup_files(&self) -> Result<(), ProviderError> {
        // create paths
        // TODO: can be done when sending files ?
        // try_join_all(
        //     options
        //         .created_paths
        //         .iter()
        //         .map(|path| node.create_path(path)),
        // )
        // .await?;

        // try_join_all(
        //     options
        //         .injected_files
        //         .iter()
        //         .map(|file| node.send_file(&file.local_path, &file.remote_path, &file.mode)),
        // )
        // .await?;

        Ok(())
    }

    async fn create_path(&self, path: &PathBuf) -> Result<(), ProviderError> {
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["mkdir", "-p", &path.to_string_lossy()],
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to created path for pod {}", &self.name),
                    err.into(),
                )
            });

        Ok(())
    }

    fn namespace_name(&self) -> String {
        self.namespace
            .upgrade()
            .and_then(|namespace| Some(namespace.name.clone()))
            .expect("namespace shouldn't be dropped")
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
        // here is just a noop op since we will receive the path
        // for the file inside the pod
        PathBuf::from(file)
    }

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
            .map_err(|err| ProviderError::RunCommandError(self.name.to_string(), err.into()))
    }

    async fn run_script(
        &self,
        options: RunScriptOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let file_name = options
            .local_script_path
            .file_name()
            .expect("file name should be present at this point")
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
        local_src: &PathBuf,
        remote_dest: &PathBuf,
        mode: &str,
    ) -> Result<(), ProviderError> {
        let data = self.filesystem.read(local_src).await.unwrap();
        self.http_client.post("");

        Ok(())
    }

    async fn receive_file(
        &self,
        remote_src: PathBuf,
        local_dest: PathBuf,
    ) -> Result<(), ProviderError> {
        Ok(())
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
