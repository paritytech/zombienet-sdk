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

#[derive(Clone)]
pub struct KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    filesystem: FS,
    k8s_client: KubernetesClient,
    inner: Arc<RwLock<KubernetesProviderInner<FS>>>,
}

pub(super) struct KubernetesProviderInner<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub(super) namespaces: HashMap<String, KubernetesNamespace<FS>>,
}

#[derive(Clone)]
pub(super) struct WeakKubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub(super) inner: Weak<RwLock<KubernetesProviderInner<FS>>>,
}

impl<FS> KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub async fn new(filesystem: FS) -> Self {
        Self {
            capabilities: ProviderCapabilities {
                requires_image: true,
                has_resources: true,
                prefix_with_full_path: false,
            },
            tmp_dir: std::env::temp_dir(),
            filesystem,
            k8s_client: KubernetesClient::new().await.unwrap(),
            inner: Arc::new(RwLock::new(KubernetesProviderInner {
                namespaces: Default::default(),
            })),
        }
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
        self.inner
            .read()
            .await
            .namespaces
            .clone()
            .into_iter()
            .map(|(id, namespace)| (id, Arc::new(namespace) as DynNamespace))
            .collect()
    }

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError> {
        let mut inner = self.inner.write().await;

        let namespace = KubernetesNamespace::new(
            &self.tmp_dir,
            &self.capabilities,
            &self.filesystem,
            &self.k8s_client,
            WeakKubernetesProvider {
                inner: Arc::downgrade(&self.inner),
            },
        )
        .await?;

        namespace.initialize().await?;

        inner
            .namespaces
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(Arc::new(namespace))
    }
}
