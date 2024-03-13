use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;
use tracing::trace;
use uuid::Uuid;

use super::node::NativeNode;
use crate::{
    constants::NAMESPACE_PREFIX,
    types::{
        GenerateFileCommand, GenerateFilesOptions, ProviderCapabilities, RunCommandOptions,
        SpawnNodeOptions,
    },
    DynNode, NativeProvider, ProviderError, ProviderNamespace, ProviderNode,
};

pub(super) struct NativeNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<NativeNamespace<FS>>,
    name: String,
    provider: Weak<NativeProvider<FS>>,
    base_dir: PathBuf,
    capabilities: ProviderCapabilities,
    filesystem: FS,
    pub(super) nodes: RwLock<HashMap<String, Arc<NativeNode<FS>>>>,
}

impl<FS> NativeNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        provider: &Weak<NativeProvider<FS>>,
        tmp_dir: &PathBuf,
        capabilities: &ProviderCapabilities,
        filesystem: &FS,
    ) -> Result<Arc<Self>, ProviderError> {
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let base_dir = PathBuf::from_iter([tmp_dir, &PathBuf::from(&name)]);
        filesystem.create_dir(&base_dir).await?;

        Ok(Arc::new_cyclic(|weak| NativeNamespace {
            weak: weak.clone(),
            provider: provider.clone(),
            name,
            base_dir,
            capabilities: capabilities.clone(),
            filesystem: filesystem.clone(),
            nodes: RwLock::new(HashMap::new()),
        }))
    }
}

#[async_trait]
impl<FS> ProviderNamespace for NativeNamespace<FS>
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

    async fn get_node_available_args(
        &self,
        (command, _image): (String, Option<String>),
    ) -> Result<String, ProviderError> {
        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(format!("temp-{}", Uuid::new_v4()), "bash".to_string())
                    .args(vec!["-c", "while :; do sleep 1; done"]),
            )
            .await?;

        let available_args_output = temp_node
            .run_command(RunCommandOptions::new(command.clone()).args(vec!["--help"]))
            .await?
            .map_err(|(_exit, status)| {
                ProviderError::NodeAvailableArgsError("".to_string(), command, status)
            })?;

        Ok(available_args_output)
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        if self.nodes.read().await.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name.clone()));
        }

        let node = NativeNode::new(
            &self.weak,
            &self.base_dir,
            &options.name,
            &options.program,
            &options.args,
            &options.env,
            &options.injected_files,
            &options.created_paths,
            &options.db_snapshot.as_ref(),
            &self.filesystem,
        )
        .await?;

        self.nodes
            .write()
            .await
            .insert(options.name.clone(), node.clone());

        Ok(node)
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        let node_name = if let Some(name) = options.temp_name {
            name
        } else {
            format!("temp-{}", Uuid::new_v4())
        };

        // we spawn a node doing nothing but looping so we can execute our commands
        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(node_name, "bash".to_string())
                    .args(vec!["-c", "while :; do sleep 1; done"])
                    .injected_files(options.injected_files),
            )
            .await?;

        for GenerateFileCommand {
            program,
            args,
            env,
            local_output_path,
        } in options.commands
        {
            trace!(
                "🏗  building file {:?} in path {} with command {} {}",
                local_output_path.as_os_str(),
                self.base_dir.to_string_lossy(),
                program,
                args.join(" ")
            );
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
        for node in self.nodes.read().await.values() {
            node.destroy().await?;
        }

        if let Some(provider) = self.provider.upgrade() {
            provider.namespaces.write().await.remove(&self.name);
        }

        Ok(())
    }
}
