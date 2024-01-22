use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use k8s_openapi::{
    api::core::v1::{Container, PodSpec, ServicePort, ServiceSpec},
    apimachinery::pkg::util::intstr::IntOrString,
};
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

    async fn create_base_dir(&self, namespace_name: &str) -> Result<PathBuf, ProviderError> {
        let base_dir = PathBuf::from_iter([&self.tmp_dir, &PathBuf::from(&namespace_name)]);
        self.filesystem.create_dir(&base_dir).await?;

        Ok(base_dir)
    }

    async fn setup_namespace(
        &self,
        namespace_name: &str,
        base_dir: &PathBuf,
    ) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([("foo".to_string(), "bar".to_string())]);

        let manifest = self
            .client
            .create_namespace(&namespace_name, labels)
            .await
            .map_err(|err| {
                ProviderError::CreateNamespaceFailed(namespace_name.to_string(), err.into())
            })?;

        let serialized_manifest = serde_yaml::to_string(&manifest).map_err(|err| {
            ProviderError::CreateNamespaceFailed(namespace_name.to_string(), err.into())
        })?;

        let dest_path = PathBuf::from_iter([base_dir, &PathBuf::from("namespace_manifest.yaml")]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await?;

        Ok(())
    }

    async fn setup_wrapper_config_map(
        &self,
        namespace_name: &str,
        base_dir: &PathBuf,
    ) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([("foo".to_string(), "bar".to_string())]);

        let manifest = self
            .client
            .create_config_map_from_file(
                &namespace_name,
                "zombie-wrapper",
                "zombie-wrapper.sh",
                include_str!("./zombie-wrapper.sh"),
                labels,
            )
            .await
            .map_err(|err| {
                ProviderError::CreateNamespaceFailed(namespace_name.to_string(), err.into())
            })?;

        let serializer_manifest = serde_yaml::to_string(&manifest).map_err(|err| {
            ProviderError::CreateNamespaceFailed(namespace_name.to_string(), err.into())
        })?;

        let dest_path = PathBuf::from_iter([
            &base_dir,
            &PathBuf::from("zombie_wrapper_config_map_manifest.yaml"),
        ]);

        self.filesystem
            .write(dest_path, serializer_manifest)
            .await?;

        Ok(())
    }

    async fn setup_file_server(&self, namespace_name: &str) -> Result<(), ProviderError> {
        let name = "fileserver".to_string();
        let labels = BTreeMap::from([(
            "app.kubernetes.io/name".to_string(),
            "fileserver".to_string(),
        )]);

        let pod_spec = PodSpec {
            hostname: Some(name.clone()),
            containers: vec![Container {
                name: name.clone(),
                image: Some("fileserver:latest".to_string()),
                image_pull_policy: Some("Always".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };

        self.client
            .create_pod(namespace_name, &name, pod_spec, labels.clone())
            .await
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

        let service_spec = ServiceSpec {
            selector: Some(labels.clone()),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                protocol: Some("TCP".to_string()),
                port: 80,
                target_port: Some(IntOrString::Int(80)),
                ..Default::default()
            }]),
            ..Default::default()
        };

        self.client
            .create_service(namespace_name, &name, service_spec, labels.clone())
            .await
            .map_err(|err| ProviderError::FileServerSetupError(err.into()))?;

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
        self.setup_file_server(&name).await?;

        let namespace = self.build_namespace(&name, base_dir);
        inner.namespaces.insert(name, namespace.clone());

        Ok(Arc::new(namespace))
    }
}
