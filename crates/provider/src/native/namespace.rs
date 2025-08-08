use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;
use tracing::{trace, warn};
use uuid::Uuid;

use super::node::{NativeNode, NativeNodeOptions};
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
        custom_base_dir: Option<&Path>,
    ) -> Result<Arc<Self>, ProviderError> {
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let base_dir = if let Some(custom_base_dir) = custom_base_dir {
            if !filesystem.exists(custom_base_dir).await {
                filesystem.create_dir_all(custom_base_dir).await?;
            } else {
                warn!(
                    "⚠️ Using and existing directory {} as base dir",
                    custom_base_dir.to_string_lossy()
                );
            }
            PathBuf::from(custom_base_dir)
        } else {
            let base_dir = PathBuf::from_iter([tmp_dir, &PathBuf::from(&name)]);
            filesystem.create_dir(&base_dir).await?;
            base_dir
        };

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

        temp_node.destroy().await?;

        Ok(available_args_output)
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        trace!("spawn node options {options:?}");

        let node = NativeNode::new(NativeNodeOptions {
            namespace: &self.weak,
            namespace_base_dir: &self.base_dir,
            name: &options.name,
            program: &options.program,
            args: &options.args,
            env: &options.env,
            startup_files: &options.injected_files,
            created_paths: &options.created_paths,
            db_snapshot: options.db_snapshot.as_ref(),
            filesystem: &self.filesystem,
        })
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
        let mut names = vec![];

        for node in self.nodes.read().await.values() {
            node.abort()
                .await
                .map_err(|err| ProviderError::DestroyNodeFailed(node.name().to_string(), err))?;
            names.push(node.name().to_string());
        }

        let mut nodes = self.nodes.write().await;
        for name in names {
            nodes.remove(&name);
        }

        if let Some(provider) = self.provider.upgrade() {
            provider.namespaces.write().await.remove(&self.name);
        }

        Ok(())
    }
}
