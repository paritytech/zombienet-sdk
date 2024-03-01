use std::{
    collections::{BTreeMap, HashMap},
    env,
    path::PathBuf,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use k8s_openapi::{
    api::core::v1::{
        Container, ContainerPort, HTTPGetAction, PodSpec, Probe, ServicePort, ServiceSpec,
    },
    apimachinery::pkg::util::intstr::IntOrString,
};
use support::fs::FileSystem;
use tokio::sync::RwLock;
use tracing::{debug, trace};
use uuid::Uuid;

use super::node::KubernetesNode;
use crate::{
    constants::NAMESPACE_PREFIX,
    kubernetes::node::KubernetesNodeOptions,
    shared::helpers::{self, running_in_ci},
    types::{
        GenerateFileCommand, GenerateFilesOptions, ProviderCapabilities, RunCommandOptions,
        SpawnNodeOptions,
    },
    DynNode, KubernetesClient, KubernetesProvider, ProviderError, ProviderNamespace, ProviderNode,
};

const FILE_SERVER_IMAGE: &str = "europe-west3-docker.pkg.dev/parity-zombienet/zombienet-public-images/zombienet-file-server:latest";

pub(super) struct KubernetesNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<KubernetesNamespace<FS>>,
    provider: Weak<KubernetesProvider<FS>>,
    name: String,
    base_dir: PathBuf,
    capabilities: ProviderCapabilities,
    k8s_client: KubernetesClient,
    filesystem: FS,
    file_server_fw_task: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(super) file_server_port: RwLock<Option<u16>>,
    pub(super) nodes: RwLock<HashMap<String, Arc<KubernetesNode<FS>>>>,
}

impl<FS> KubernetesNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        provider: &Weak<KubernetesProvider<FS>>,
        tmp_dir: &PathBuf,
        capabilities: &ProviderCapabilities,
        k8s_client: &KubernetesClient,
        filesystem: &FS,
    ) -> Result<Arc<Self>, ProviderError> {
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let base_dir = PathBuf::from_iter([tmp_dir, &PathBuf::from(&name)]);
        filesystem.create_dir(&base_dir).await?;

        let namespace = Arc::new_cyclic(|weak| KubernetesNamespace {
            weak: weak.clone(),
            provider: provider.clone(),
            name,
            base_dir,
            capabilities: capabilities.clone(),
            filesystem: filesystem.clone(),
            k8s_client: k8s_client.clone(),
            file_server_port: RwLock::new(None),
            file_server_fw_task: RwLock::new(None),
            nodes: RwLock::new(HashMap::new()),
        });

        namespace.initialize().await?;

        Ok(namespace)
    }

    pub(super) async fn initialize(&self) -> Result<(), ProviderError> {
        self.initialize_k8s().await?;
        self.initialize_file_server().await?;

        self.setup_script_config_map(
            "zombie-wrapper",
            include_str!("./scripts/zombie-wrapper.sh"),
            "zombie_wrapper_config_map_manifest.yaml",
            // TODO: add correct labels
            BTreeMap::new(),
        )
        .await?;

        self.setup_script_config_map(
            "helper-binaries-downloader",
            include_str!("./scripts/helper-binaries-downloader.sh"),
            "helper_binaries_downloader_config_map_manifest.yaml",
            // TODO: add correct labels
            BTreeMap::new(),
        )
        .await?;

        Ok(())
    }

    async fn initialize_k8s(&self) -> Result<(), ProviderError> {
        // TODO (javier): check with Hamid if we are using this labels in any scheduling logic.
        let labels = BTreeMap::from([
            (
                "jobId".to_string(),
                env::var("CI_JOB_ID").unwrap_or("".to_string()),
            ),
            (
                "projectName".to_string(),
                env::var("CI_PROJECT_NAME").unwrap_or("".to_string()),
            ),
            (
                "projectId".to_string(),
                env::var("CI_PROJECT_ID").unwrap_or("".to_string()),
            ),
        ]);

        let manifest = self
            .k8s_client
            .create_namespace(&self.name, labels)
            .await
            .map_err(|err| {
                ProviderError::CreateNamespaceFailed(self.name.to_string(), err.into())
            })?;

        let serialized_manifest = serde_yaml::to_string(&manifest).map_err(|err| {
            ProviderError::CreateNamespaceFailed(self.name.to_string(), err.into())
        })?;

        let dest_path =
            PathBuf::from_iter([&self.base_dir, &PathBuf::from("namespace_manifest.yaml")]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await?;

        // Ensure namespace isolation and minimal resources IFF we are running in CI
        if running_in_ci() {
            self.initialize_static_resources().await?
        }
        Ok(())
    }

    async fn initialize_static_resources(&self) -> Result<(), ProviderError> {
        let np_manifest = helpers::apply_replacements(
            include_str!("./static-configs/namespace-network-policy.yaml"),
            &HashMap::from([("namespace", self.name())]),
        );

        // Apply NetworkPolicy manifest
        self.k8s_client
            .create_static_resource(&self.name, &np_manifest)
            .await
            .map_err(|err| {
                ProviderError::CreateNamespaceFailed(self.name.to_string(), err.into())
            })?;

        // Apply LimitRange manifest
        self.k8s_client
            .create_static_resource(
                &self.name,
                include_str!("./static-configs/baseline-resources.yaml"),
            )
            .await
            .map_err(|err| {
                ProviderError::CreateNamespaceFailed(self.name.to_string(), err.into())
            })?;
        Ok(())
    }

    async fn initialize_file_server(&self) -> Result<(), ProviderError> {
        let name = "fileserver".to_string();
        let labels = BTreeMap::from([
            ("app.kubernetes.io/name".to_string(), name.clone()),
            (
                "x-infra-instance".to_string(),
                env::var("X_INFRA_INSTANCE").unwrap_or("ondemand".to_string()),
            ),
        ]);

        let pod_spec = PodSpec {
            hostname: Some(name.clone()),
            containers: vec![Container {
                name: name.clone(),
                image: Some(FILE_SERVER_IMAGE.to_string()),
                image_pull_policy: Some("Always".to_string()),
                ports: Some(vec![ContainerPort {
                    container_port: 80,
                    ..Default::default()
                }]),
                startup_probe: Some(Probe {
                    http_get: Some(HTTPGetAction {
                        path: Some("/".to_string()),
                        port: IntOrString::Int(80),
                        ..Default::default()
                    }),
                    initial_delay_seconds: Some(1),
                    period_seconds: Some(2),
                    failure_threshold: Some(3),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            restart_policy: Some("OnFailure".into()),
            ..Default::default()
        };

        let pod_manifest = self
            .k8s_client
            .create_pod(&self.name, &name, pod_spec, labels.clone())
            .await
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        // TODO: remove duplication across methods
        let pod_serialized_manifest = serde_yaml::to_string(&pod_manifest)
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        let pod_dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from("file_server_pod_manifest.yaml"),
        ]);

        self.filesystem
            .write(pod_dest_path, pod_serialized_manifest)
            .await?;

        let service_spec = ServiceSpec {
            selector: Some(labels.clone()),
            ports: Some(vec![ServicePort {
                port: 80,
                ..Default::default()
            }]),
            ..Default::default()
        };

        let service_manifest = self
            .k8s_client
            .create_service(&self.name, &name, service_spec, labels)
            .await
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        let serialized_service_manifest = serde_yaml::to_string(&service_manifest)
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        let service_dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from("file_server_service_manifest.yaml"),
        ]);

        self.filesystem
            .write(service_dest_path, serialized_service_manifest)
            .await?;

        let (port, task) = self
            .k8s_client
            .create_pod_port_forward(&self.name, &name, 0, 80)
            .await
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        *self.file_server_port.write().await = Some(port);
        *self.file_server_fw_task.write().await = Some(task);

        Ok(())
    }

    async fn setup_script_config_map(
        &self,
        name: &str,
        script_contents: &str,
        local_manifest_name: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<(), ProviderError> {
        let manifest = self
            .k8s_client
            .create_config_map_from_file(
                &self.name,
                name,
                &format!("{name}.sh"),
                script_contents,
                labels,
            )
            .await
            .map_err(|err| {
                ProviderError::CreateNamespaceFailed(self.name.to_string(), err.into())
            })?;

        let serializer_manifest = serde_yaml::to_string(&manifest).map_err(|err| {
            ProviderError::CreateNamespaceFailed(self.name.to_string(), err.into())
        })?;

        let dest_path = PathBuf::from_iter([&self.base_dir, &PathBuf::from(local_manifest_name)]);

        self.filesystem
            .write(dest_path, serializer_manifest)
            .await?;

        Ok(())
    }

}

#[async_trait]
impl<FS> ProviderNamespace for KubernetesNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    async fn nodes(&self) -> HashMap<String, DynNode> {
        self.nodes
            .read()
            .await
            .iter()
            .map(|(name, node)| (name.clone(), node.clone() as DynNode))
            .collect()
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        trace!("spawn option {:?}", options);
        if self.nodes.read().await.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name.clone()));
        }

        let node = KubernetesNode::new(KubernetesNodeOptions {
            namespace: &self.weak,
            namespace_base_dir: &self.base_dir,
            name: &options.name,
            image: options.image.as_ref(),
            program: &options.program,
            args: &options.args,
            env: &options.env,
            startup_files: &options.injected_files,
            resources: options.resources.as_ref(),
            db_snapshot: options.db_snapshot.as_ref(),
            k8s_client: &self.k8s_client,
            filesystem: &self.filesystem,
        })
        .await?;

        self.nodes
            .write()
            .await
            .insert(node.name().to_string(), node.clone());

        Ok(node)
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        debug!("options {:#?}", options);

        let node_name = options
            .temp_name
            .unwrap_or_else(|| format!("temp-{}", Uuid::new_v4()));
        let node_image = options
            .image
            .expect("image should be present when generating files with kubernetes provider");

        // run dummy command in new pod
        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(node_name, "cat".to_string())
                    .injected_files(options.injected_files)
                    .image(node_image),
            )
            .await?;

        for GenerateFileCommand {
            program,
            args,
            env,
            local_output_path,
        } in options.commands
        {
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
                .await?
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
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        let _ = self
            .k8s_client
            .delete_namespace(&self.name)
            .await
            .map_err(|err| ProviderError::DeleteNamespaceFailed(self.name.clone(), err.into()))?;

        if let Some(provider) = self.provider.upgrade() {
            provider.namespaces.write().await.remove(&self.name);
        }

        Ok(())
    }
}
