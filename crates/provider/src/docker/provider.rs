use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;

use super::{client::DockerClient, namespace::DockerNamespace};
use crate::{
    types::ProviderCapabilities, DynNamespace, Provider, ProviderError, ProviderNamespace,
};

const PROVIDER_NAME: &str = "docker";

pub struct DockerProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<DockerProvider<FS>>,
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    docker_client: DockerClient,
    filesystem: FS,
    pub(super) namespaces: RwLock<HashMap<String, Arc<DockerNamespace<FS>>>>,
}

impl<FS> DockerProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub async fn new(filesystem: FS) -> Arc<Self> {
        let docker_client = DockerClient::new().await.unwrap();

        Arc::new_cyclic(|weak| DockerProvider {
            weak: weak.clone(),
            capabilities: ProviderCapabilities {
                requires_image: true,
                has_resources: false,
                prefix_with_full_path: false,
                use_default_ports_in_cmd: true,
            },
            tmp_dir: std::env::temp_dir(),
            docker_client,
            filesystem,
            namespaces: RwLock::new(HashMap::new()),
        })
    }

    pub fn tmp_dir(mut self, tmp_dir: impl Into<PathBuf>) -> Self {
        self.tmp_dir = tmp_dir.into();
        self
    }
}

#[async_trait]
impl<FS> Provider for DockerProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    async fn namespaces(&self) -> HashMap<String, DynNamespace> {
        self.namespaces
            .read()
            .await
            .iter()
            .map(|(name, namespace)| (name.clone(), namespace.clone() as DynNamespace))
            .collect()
    }

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError> {
        let namespace = DockerNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
            &self.docker_client,
            &self.filesystem,
            None,
        )
        .await?;

        self.namespaces
            .write()
            .await
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(namespace)
    }

    async fn create_namespace_with_base_dir(
        &self,
        base_dir: &Path,
    ) -> Result<DynNamespace, ProviderError> {
        let namespace = DockerNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
            &self.docker_client,
            &self.filesystem,
            Some(base_dir),
        )
        .await?;

        self.namespaces
            .write()
            .await
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(namespace)
    }
}
