use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;

use super::{client::KubernetesClient, namespace::KubernetesNamespace};
use crate::{
    types::ProviderCapabilities, DynNamespace, Provider, ProviderError, ProviderNamespace,
};

pub struct KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<KubernetesProvider<FS>>,
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    k8s_client: KubernetesClient,
    filesystem: FS,
    pub(super) namespaces: RwLock<HashMap<String, Arc<KubernetesNamespace<FS>>>>,
}

impl<FS> KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub async fn new(filesystem: FS) -> Arc<Self> {
        let k8s_client = KubernetesClient::new().await.unwrap();

        Arc::new_cyclic(|weak| KubernetesProvider {
            weak: weak.clone(),
            capabilities: ProviderCapabilities {
                requires_image: true,
                has_resources: true,
                prefix_with_full_path: false,
            },
            tmp_dir: std::env::temp_dir(),
            k8s_client,
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
impl<FS> Provider for KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
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
        let namespace = KubernetesNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
            &self.k8s_client,
            &self.filesystem,
        )
        .await?;

        self.namespaces
            .write()
            .await
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(namespace)
    }
}
