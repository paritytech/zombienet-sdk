use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::{
    client::KubernetesClient,
    namespace::{KubernetesNamespace, KubernetesNamespaceInner},
};
use crate::{
    constants::NAMESPACE_PREFIX, types::ProviderCapabilities, DynNamespace, Provider, ProviderError,
};

#[derive(Clone)]
pub struct KubernetesProvider<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    filesystem: FS,
    client: KC,
    inner: Arc<RwLock<KubernetesProviderInner<FS, KC>>>,
}

pub(super) struct KubernetesProviderInner<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) namespaces: HashMap<String, KubernetesNamespace<FS, KC>>,
}

#[derive(Clone)]
pub(super) struct WeakKubernetesProvider<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) inner: Weak<RwLock<KubernetesProviderInner<FS, KC>>>,
}

impl<FS, KC> KubernetesProvider<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub fn new(filesystem: FS, client: KC) -> Self {
        Self {
            capabilities: ProviderCapabilities {
                requires_image: true,
                has_resources: true,
                prefix_with_full_path: false,
            },
            tmp_dir: std::env::temp_dir(),
            filesystem,
            client,
            inner: Arc::new(RwLock::new(KubernetesProviderInner {
                namespaces: Default::default(),
            })),
        }
    }

    pub fn tmp_dir(mut self, tmp_dir: impl Into<PathBuf>) -> Self {
        self.tmp_dir = tmp_dir.into();
        self
    }

    async fn create_base_dir(&self, name: &str) -> Result<PathBuf, ProviderError> {
        let base_dir = PathBuf::from_iter([&self.tmp_dir, &PathBuf::from(&name)]);
        self.filesystem.create_dir(&base_dir).await?;

        Ok(base_dir)
    }

    async fn setup_namespace(&self, name: &str, base_dir: &PathBuf) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([("foo".to_string(), "bar".to_string())]);

        let manifest = self
            .client
            .create_namespace(&name, labels)
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(name.to_string(), err.into()))?;

        let serialized_manifest = serde_yaml::to_string(&manifest)
            .map_err(|err| ProviderError::CreateNamespaceFailed(name.to_string(), err.into()))?;

        let dest_path = PathBuf::from_iter([base_dir, &PathBuf::from("namespace_manifest.yaml")]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await?;

        Ok(())
    }

    async fn setup_wrapper_config_map(
        &self,
        name: &str,
        base_dir: &PathBuf,
    ) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([("foo".to_string(), "bar".to_string())]);

        let manifest = self
            .client
            .create_config_map_from_file(
                &name,
                "zombie-wrapper",
                "zombie-wrapper.sh",
                include_str!("./zombie-wrapper.sh"),
                labels,
            )
            .await
            .map_err(|err| ProviderError::CreateNamespaceFailed(name.to_string(), err.into()))?;

        let serializer_manifest = serde_yaml::to_string(&manifest)
            .map_err(|err| ProviderError::CreateNamespaceFailed(name.to_string(), err.into()))?;

        let dest_path = PathBuf::from_iter([
            &base_dir,
            &PathBuf::from("zombie_wrapper_config_map_manifest.yaml"),
        ]);

        self.filesystem
            .write(dest_path, serializer_manifest)
            .await?;

        Ok(())
    }

    fn build_namespace(&self, name: &str, base_dir: PathBuf) -> KubernetesNamespace<FS, KC> {
        KubernetesNamespace {
            name: name.to_string(),
            base_dir,
            capabilities: self.capabilities.clone(),
            filesystem: self.filesystem.clone(),
            client: self.client.clone(),
            provider: WeakKubernetesProvider {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(KubernetesNamespaceInner {
                nodes: Default::default(),
            })),
        }
    }
}

#[async_trait]
impl<FS, KC> Provider for KubernetesProvider<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    KC: KubernetesClient<FS> + Send + Sync + Clone + 'static,
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
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());

        let base_dir = self.create_base_dir(&name).await?;

        self.setup_namespace(&name, &base_dir).await?;
        self.setup_wrapper_config_map(&name, &base_dir).await?;

        let namespace = self.build_namespace(&name, base_dir);
        inner.namespaces.insert(name, namespace.clone());

        Ok(Arc::new(namespace))
    }
}
