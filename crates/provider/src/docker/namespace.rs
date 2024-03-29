use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::shared::constants::THIS_IS_A_BUG;
use support::fs::FileSystem;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::{
    constants::NAMESPACE_PREFIX,
    docker::node::DockerNodeOptions,
    types::{
        GenerateFileCommand, GenerateFilesOptions, ProviderCapabilities, RunCommandOptions,
        SpawnNodeOptions,
    },
    DynNode, ProviderError, ProviderNamespace, ProviderNode,
};

use super::{
    client::{ContainerRunOptions, DockerClient},
    node::DockerNode,
    DockerProvider,
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
    ) -> Result<Arc<Self>, ProviderError> {
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let base_dir = PathBuf::from_iter([tmp_dir, &PathBuf::from(&name)]);
        filesystem.create_dir(&base_dir).await?;

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

    async fn initialize(&self) -> Result<(), ProviderError> {
        self.initialize_zombie_wrapper_volume().await?;
        self.initialize_helper_binaries_volume().await?;

        Ok(())
    }

    async fn initialize_zombie_wrapper_volume(&self) -> Result<(), ProviderError> {
        let local_zombie_wrapper_path =
            PathBuf::from_iter([&self.base_dir, &PathBuf::from("zombie-wrapper.sh")]);

        self.filesystem
            .write(
                &local_zombie_wrapper_path,
                include_str!("../shared/scripts/zombie-wrapper.sh"),
            )
            .await?;

        let zombie_wrapper_volume_name = format!("{}-zombie-wrapper", self.name);

        self.docker_client
            .create_volume(&zombie_wrapper_volume_name)
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        // copy script to volume
        self.docker_client
            .container_run(
                ContainerRunOptions::new(
                    "alpine:latest",
                    vec!["cp", "/zombie-wrapper.sh", "/scripts"],
                )
                .volume_mounts(HashMap::from([
                    (
                        local_zombie_wrapper_path.to_string_lossy().as_ref(),
                        "/zombie-wrapper.sh",
                    ),
                    (&zombie_wrapper_volume_name, "/scripts"),
                ]))
                .rm(),
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

        let helper_binaries_volume_name = format!("{}-helper-binaries", self.name);

        self.docker_client
            .create_volume(&helper_binaries_volume_name)
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(self.name.clone(), err.into()))?;

        // download binaries to volume
        self.docker_client
            .container_run(
                ContainerRunOptions::new(
                    "alpine:latest",
                    vec!["ash", "/helper-binaries-downloader.sh"],
                )
                .volume_mounts(HashMap::from([
                    (
                        local_helper_binaries_downloader_path
                            .to_string_lossy()
                            .as_ref(),
                        "/helper-binaries-downloader.sh",
                    ),
                    (&helper_binaries_volume_name, "/helpers"),
                ]))
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

    pub async fn delete_on_drop(&self, delete_on_drop: bool) {
        *self.delete_on_drop.lock().await = delete_on_drop;
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
        self.delete_on_drop(false).await;
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

        Ok(available_args_output)
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        trace!("spawn option {:?}", options);
        if self.nodes.read().await.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name.clone()));
        }

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
        })
        .await?;

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
        todo!()
    }
}
