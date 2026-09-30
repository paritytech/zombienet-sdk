use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;

use super::{client::KubernetesClient, namespace::KubernetesNamespace};
use crate::{
    shared::helpers::extract_namespace_info, types::ProviderCapabilities, DynNamespace, Provider,
    ProviderError, ProviderNamespace,
};

pub const PROVIDER_NAME: &str = "k8s";

pub struct KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<KubernetesProvider<FS>>,
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    k8s_client: KubernetesClient,
    filesystem: FS,
    // Pre-existing namespace to spawn into, instead of creating one.
    existing_namespace: Option<String>,
    pub(super) namespaces: RwLock<HashMap<String, Arc<KubernetesNamespace<FS>>>>,
}

impl<FS> KubernetesProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub async fn new(filesystem: FS) -> Arc<Self> {
        Self::build(filesystem, None).await
    }

    /// Spawns into `namespace`, which must already exist. Zombienet won't create it
    /// nor delete it on drop, but `destroy()` still deletes it.
    pub async fn new_in_namespace(filesystem: FS, namespace: impl Into<String>) -> Arc<Self> {
        Self::build(filesystem, Some(namespace.into())).await
    }

    async fn build(filesystem: FS, existing_namespace: Option<String>) -> Arc<Self> {
        let k8s_client = KubernetesClient::new().await.unwrap();
        Self::with_client(filesystem, k8s_client, existing_namespace)
    }

    fn with_client(
        filesystem: FS,
        k8s_client: KubernetesClient,
        existing_namespace: Option<String>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak| KubernetesProvider {
            weak: weak.clone(),
            capabilities: ProviderCapabilities {
                requires_image: true,
                has_resources: true,
                prefix_with_full_path: false,
                use_default_ports_in_cmd: true,
            },
            tmp_dir: std::env::temp_dir(),
            k8s_client,
            filesystem,
            existing_namespace,
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
        let namespace = KubernetesNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
            &self.k8s_client,
            &self.filesystem,
            None,
            self.existing_namespace.as_deref(),
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
        let namespace = KubernetesNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
            &self.k8s_client,
            &self.filesystem,
            Some(base_dir),
            self.existing_namespace.as_deref(),
        )
        .await?;

        self.namespaces
            .write()
            .await
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(namespace)
    }

    async fn create_namespace_from_json(
        &self,
        json_value: &serde_json::Value,
    ) -> Result<DynNamespace, ProviderError> {
        let (base_dir, name) = extract_namespace_info(json_value)?;

        let namespace = KubernetesNamespace::attach_to_live(
            &self.weak,
            &self.capabilities,
            &self.k8s_client,
            &self.filesystem,
            &base_dir,
            &name,
        )
        .await?;

        self.namespaces
            .write()
            .await
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(namespace)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString};

    use mockito::{Matcher, Mock, Server, ServerGuard};
    use support::fs::in_memory::{InMemoryFile, InMemoryFileSystem};

    use super::*;

    /// Provider talking to `server`. Unmocked requests get mockito's 501, so
    /// spawning fails right after the namespace step. kube appends a query string,
    /// hence `match_query` on exact-path mocks.
    fn provider(
        server: &ServerGuard,
        existing_namespace: Option<&str>,
    ) -> Arc<KubernetesProvider<InMemoryFileSystem>> {
        // In-memory `create_dir` needs every ancestor of the tmp dir to exist. Going
        // through `components()` drops the trailing `/` `$TMPDIR` may have.
        let tmp_dir: PathBuf = std::env::temp_dir().components().collect();
        let files: HashMap<OsString, InMemoryFile> = tmp_dir
            .ancestors()
            .map(|dir| (dir.as_os_str().to_owned(), InMemoryFile::dir()))
            .collect();
        let config = kube::Config::new(server.url().parse().unwrap());
        let client = KubernetesClient::from_inner(kube::Client::try_from(config).unwrap());

        KubernetesProvider::with_client(
            InMemoryFileSystem::new(files),
            client,
            existing_namespace.map(String::from),
        )
    }

    async fn mock_create_namespace(server: &mut ServerGuard, times: usize) -> Mock {
        server
            .mock("POST", "/api/v1/namespaces")
            .match_query(Matcher::Any)
            .with_status(201)
            .with_body(r#"{"apiVersion":"v1","kind":"Namespace","metadata":{"name":"mock"}}"#)
            .expect(times)
            .create_async()
            .await
    }

    async fn mock_delete_namespace(server: &mut ServerGuard, path: Matcher, times: usize) -> Mock {
        server
            .mock("DELETE", path)
            .with_status(200)
            .expect(times)
            .create_async()
            .await
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn existing_namespace_is_neither_created_nor_deleted() {
        let mut server = Server::new_async().await;
        let create = mock_create_namespace(&mut server, 0).await;
        let delete = mock_delete_namespace(&mut server, Matcher::Any, 0).await;
        let file_server = server
            .mock("POST", "/api/v1/namespaces/adopted/pods")
            .match_query(Matcher::Any)
            .with_status(500)
            .create_async()
            .await;

        let provider = provider(&server, Some("adopted"));
        assert!(provider.create_namespace().await.is_err());
        drop(provider);

        file_server.assert_async().await;
        create.assert_async().await;
        delete.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn own_namespace_is_created_and_deleted_on_drop() {
        let mut server = Server::new_async().await;
        let create = mock_create_namespace(&mut server, 1).await;
        let delete = mock_delete_namespace(
            &mut server,
            Matcher::Regex("^/api/v1/namespaces/zombie-".into()),
            1,
        )
        .await;

        let provider = provider(&server, None);
        assert!(provider.create_namespace().await.is_err());
        drop(provider);

        create.assert_async().await;
        delete.assert_async().await;
    }
}
