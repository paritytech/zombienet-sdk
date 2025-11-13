use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    thread,
};

use async_trait::async_trait;
use support::{constants::THIS_IS_A_BUG, fs::FileSystem};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, trace, warn};
use uuid::Uuid;

use super::{
    client::{ContainerRunOptions, DockerClient},
    node::DockerNode,
    DockerProvider,
};
use crate::{
    constants::NAMESPACE_PREFIX,
    docker::node::{DeserializableDockerNodeOptions, DockerNodeOptions},
    shared::helpers::extract_execution_result,
    types::{
        GenerateFileCommand, GenerateFilesOptions, ProviderCapabilities, RunCommandOptions,
        SpawnNodeOptions,
    },
    DynNode, ProviderError, ProviderNamespace, ProviderNode,
};

pub struct DockerNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<DockerNamespace<FS>>,
    #[allow(dead_code)]
    provider: Weak<DockerProvider<FS>>,
    name: String,
    base_dir: PathBuf,
    capabilities: ProviderCapabilities,
    docker_client: DockerClient,
    filesystem: FS,
    delete_on_drop: Arc<Mutex<bool>>,
    pub(super) nodes: RwLock<HashMap<String, Arc<DockerNode<FS>>>>,
}

impl<FS> DockerNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        provider: &Weak<DockerProvider<FS>>,
        tmp_dir: &PathBuf,
        capabilities: &ProviderCapabilities,
        docker_client: &DockerClient,
        filesystem: &FS,
        custom_base_dir: Option<&Path>,
    ) -> Result<Arc<Self>, ProviderError> {
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let base_dir = if let Some(custom_base_dir) = custom_base_dir {
            if !filesystem.exists(custom_base_dir).await {
                filesystem.create_dir(custom_base_dir).await?;
            } else {
                warn!(
                    "⚠️  Using and existing directory {} as base dir",
                    custom_base_dir.to_string_lossy()
                );
            }
            PathBuf::from(custom_base_dir)
        } else {
            let base_dir = PathBuf::from_iter([tmp_dir, &PathBuf::from(&name)]);
            filesystem.create_dir(&base_dir).await?;
            base_dir
        };

        let namespace = Arc::new_cyclic(|weak| DockerNamespace {
            weak: weak.clone(),
            provider: provider.clone(),
            name,
            base_dir,
            capabilities: capabilities.clone(),
            filesystem: filesystem.clone(),
            docker_client: docker_client.clone(),
            nodes: RwLock::new(HashMap::new()),
            delete_on_drop: Arc::new(Mutex::new(true)),
        });

        namespace.initialize().await?;

        Ok(namespace)
    }

    pub(super) async fn attach_to_live(
        provider: &Weak<DockerProvider<FS>>,
        capabilities: &ProviderCapabilities,
        docker_client: &DockerClient,
        filesystem: &FS,
        custom_base_dir: &Path,
        name: &str,
    ) -> Result<Arc<Self>, ProviderError> {
        let base_dir = custom_base_dir.to_path_buf();

        let namespace = Arc::new_cyclic(|weak| DockerNamespace {
            weak: weak.clone(),
            provider: provider.clone(),
            name: name.to_owned(),
            base_dir,
            capabilities: capabilities.clone(),
            filesystem: filesystem.clone(),
            docker_client: docker_client.clone(),
            nodes: RwLock::new(HashMap::new()),
            delete_on_drop: Arc::new(Mutex::new(false)),
        });

        Ok(namespace)
    }

    async fn initialize(&self) -> Result<(), ProviderError> {
        // let ns_scripts_shared =  PathBuf::from_iter([&self.base_dir, &PathBuf::from("shared-scripts")]);
        // self.filesystem.create_dir(&ns_scripts_shared).await?;
        self.initialize_zombie_scripts_volume().await?;
        self.initialize_helper_binaries_volume().await?;

        Ok(())
    }

    async fn initialize_zombie_scripts_volume(&self) -> Result<(), ProviderError> {
        let local_zombie_wrapper_path =
            PathBuf::from_iter([&self.base_dir, &PathBuf::from("zombie-wrapper.sh")]);

        self.filesystem
            .write(
                &local_zombie_wrapper_path,
                include_str!("../shared/scripts/zombie-wrapper.sh"),
            )
            .await?;

        let local_helper_binaries_downloader_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from("helper-binaries-downloader.sh"),
        ]);

        self.filesystem
            .write(
                &local_helper_binaries_downloader_path,
                include_str!("../shared/scripts/helper-binaries-downloader.sh"),
            )
            .await?;

        let zombie_wrapper_volume_name = format!("{}-zombie-wrapper", self.name);
        let zombie_wrapper_container_name = format!("{}-scripts", self.name);

        self.docker_client
            .create_volume(&zombie_wrapper_volume_name)
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        self.docker_client
            .container_create(
                ContainerRunOptions::new("alpine:latest", vec!["tail", "-f", "/dev/null"])
                    .volume_mounts(HashMap::from([(
                        zombie_wrapper_volume_name.as_str(),
                        "/scripts",
                    )]))
                    .name(&zombie_wrapper_container_name)
                    .rm(),
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        // copy the scripts
        self.docker_client
            .container_cp(
                &zombie_wrapper_container_name,
                &local_zombie_wrapper_path,
                &PathBuf::from("/scripts/zombie-wrapper.sh"),
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        self.docker_client
            .container_cp(
                &zombie_wrapper_container_name,
                &local_helper_binaries_downloader_path,
                &PathBuf::from("/scripts/helper-binaries-downloader.sh"),
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        // set permissions for rwx on whole volume recursively
        self.docker_client
            .container_run(
                ContainerRunOptions::new("alpine:latest", vec!["chmod", "-R", "777", "/scripts"])
                    .volume_mounts(HashMap::from([(
                        zombie_wrapper_volume_name.as_ref(),
                        "/scripts",
                    )]))
                    .rm(),
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        Ok(())
    }

    async fn initialize_helper_binaries_volume(&self) -> Result<(), ProviderError> {
        let helper_binaries_volume_name = format!("{}-helper-binaries", self.name);
        let zombie_wrapper_volume_name = format!("{}-zombie-wrapper", self.name);

        self.docker_client
            .create_volume(&helper_binaries_volume_name)
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        // download binaries to volume
        self.docker_client
            .container_run(
                ContainerRunOptions::new(
                    "alpine:latest",
                    vec!["ash", "/scripts/helper-binaries-downloader.sh"],
                )
                .volume_mounts(HashMap::from([
                    (
                        helper_binaries_volume_name.as_str(),
                        "/helpers",
                    ),
                    (
                        zombie_wrapper_volume_name.as_ref(),
                        "/scripts",
                    )
                ]))
                // wait until complete
                .detach(false)
                .rm(),
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        // set permissions for rwx on whole volume recursively
        self.docker_client
            .container_run(
                ContainerRunOptions::new("alpine:latest", vec!["chmod", "-R", "777", "/helpers"])
                    .volume_mounts(HashMap::from([(
                        helper_binaries_volume_name.as_ref(),
                        "/helpers",
                    )]))
                    .rm(),
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        Ok(())
    }

    pub async fn set_delete_on_drop(&self, delete_on_drop: bool) {
        *self.delete_on_drop.lock().await = delete_on_drop;
    }

    pub async fn delete_on_drop(&self) -> bool {
        if let Ok(delete_on_drop) = self.delete_on_drop.try_lock() {
            *delete_on_drop
        } else {
            // if we can't lock just remove the ns
            true
        }
    }
}

#[async_trait]
impl<FS> ProviderNamespace for DockerNamespace<FS>
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

    async fn detach(&self) {
        self.set_delete_on_drop(false).await;
    }

    async fn is_detached(&self) -> bool {
        self.delete_on_drop().await
    }

    async fn nodes(&self) -> HashMap<String, DynNode> {
        self.nodes
            .read()
            .await
            .iter()
            .map(|(name, node)| (name.clone(), node.clone() as DynNode))
            .collect()
    }

    async fn get_node_available_args(
        &self,
        (command, image): (String, Option<String>),
    ) -> Result<String, ProviderError> {
        let node_image = image.expect(&format!("image should be present when getting node available args with docker provider {THIS_IS_A_BUG}"));

        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(format!("temp-{}", Uuid::new_v4()), "cat".to_string())
                    .image(node_image.clone()),
            )
            .await?;

        let available_args_output = temp_node
            .run_command(RunCommandOptions::new(command.clone()).args(vec!["--help"]))
            .await?
            .map_err(|(_exit, status)| {
                ProviderError::NodeAvailableArgsError(node_image, command, status)
            })?;

        temp_node.destroy().await?;

        Ok(available_args_output)
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        debug!("spawn option {:?}", options);

        let node = DockerNode::new(DockerNodeOptions {
            namespace: &self.weak,
            namespace_base_dir: &self.base_dir,
            name: &options.name,
            image: options.image.as_ref(),
            program: &options.program,
            args: &options.args,
            env: &options.env,
            startup_files: &options.injected_files,
            db_snapshot: options.db_snapshot.as_ref(),
            docker_client: &self.docker_client,
            container_name: format!("{}-{}", self.name, options.name),
            filesystem: &self.filesystem,
            port_mapping: options.port_mapping.as_ref().unwrap_or(&HashMap::default()),
        })
        .await?;

        self.nodes
            .write()
            .await
            .insert(node.name().to_string(), node.clone());

        Ok(node)
    }

    async fn spawn_node_from_json(
        &self,
        json_value: &serde_json::Value,
    ) -> Result<DynNode, ProviderError> {
        let deserializable: DeserializableDockerNodeOptions =
            serde_json::from_value(json_value.clone())?;
        let options = DockerNodeOptions::from_deserializable(
            &deserializable,
            &self.weak,
            &self.base_dir,
            &self.docker_client,
            &self.filesystem,
        );

        let node = DockerNode::attach_to_live(options).await?;

        self.nodes
            .write()
            .await
            .insert(node.name().to_string(), node.clone());

        Ok(node)
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        debug!("generate files options {options:#?}");

        let node_name = options
            .temp_name
            .unwrap_or_else(|| format!("temp-{}", Uuid::new_v4()));
        let node_image = options.image.expect(&format!(
            "image should be present when generating files with docker provider {THIS_IS_A_BUG}"
        ));

        // run dummy command in a new container
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

            let contents = extract_execution_result(
                &temp_node,
                RunCommandOptions { program, args, env },
                options.expected_path.as_ref(),
            )
            .await?;
            self.filesystem
                .write(local_output_full_path, contents)
                .await
                .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?;
        }

        temp_node.destroy().await
    }

    async fn static_setup(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        let _ = self
            .docker_client
            .namespaced_containers_rm(&self.name)
            .await
            .map_err(|err| ProviderError::DeleteNamespaceFailed(self.name.clone(), err.into()))?;

        if let Some(provider) = self.provider.upgrade() {
            provider.namespaces.write().await.remove(&self.name);
        }

        Ok(())
    }
}

impl<FS> Drop for DockerNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    fn drop(&mut self) {
        let ns_name = self.name.clone();
        if let Ok(delete_on_drop) = self.delete_on_drop.try_lock() {
            if *delete_on_drop {
                let client = self.docker_client.clone();
                let provider = self.provider.upgrade();

                let handler = thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async move {
                        trace!("🧟 deleting ns {ns_name} from cluster");
                        let _ = client.namespaced_containers_rm(&ns_name).await;
                        trace!("✅ deleted");
                    });
                });

                if handler.join().is_ok() {
                    if let Some(provider) = provider {
                        if let Ok(mut p) = provider.namespaces.try_write() {
                            p.remove(&self.name);
                        } else {
                            warn!(
                                "⚠️  Can not acquire write lock to the provider, ns {} not removed",
                                self.name
                            );
                        }
                    }
                }
            } else {
                trace!("⚠️ leaking ns {ns_name} in cluster");
            }
        };
    }
}
