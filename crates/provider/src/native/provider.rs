use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::{fs::FileSystem, process::ProcessManager};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::namespace::{NativeNamespace, NativeNamespaceInner};
use crate::{
    constants::NAMESPACE_PREFIX, types::ProviderCapabilities, DynNamespace, Provider, ProviderError,
};

#[derive(Clone)]
pub struct NativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    filesystem: FS,
    process_manager: PM,
    inner: Arc<RwLock<NativeProviderInner<FS, PM>>>,
}

pub(super) struct NativeProviderInner<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub(super) namespaces: HashMap<String, NativeNamespace<FS, PM>>,
}

#[derive(Clone)]
pub(super) struct WeakNativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub(super) inner: Weak<RwLock<NativeProviderInner<FS, PM>>>,
}

impl<FS, PM> NativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub fn new(filesystem: FS, process_manager: PM) -> Self {
        Self {
            capabilities: ProviderCapabilities::new(),
            // NOTE: temp_dir in linux return `/tmp` but on mac something like
            //  `/var/folders/rz/1cyx7hfj31qgb98d8_cg7jwh0000gn/T/`, having
            // one `trailing slash` and the other no can cause issues if
            // you try to build a fullpath by concatenate. Use Pathbuf to prevent the issue.
            tmp_dir: std::env::temp_dir(),
            filesystem,
            process_manager,
            inner: Arc::new(RwLock::new(NativeProviderInner {
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
impl<FS, PM> Provider for NativeProvider<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    PM: ProcessManager + Send + Sync + Clone + 'static,
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
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let mut inner = self.inner.write().await;

        let base_dir = PathBuf::from_iter([&self.tmp_dir, &PathBuf::from(&name)]);
        self.filesystem.create_dir(&base_dir).await?;

        let namespace = NativeNamespace {
            name: name.clone(),
            base_dir,
            filesystem: self.filesystem.clone(),
            process_manager: self.process_manager.clone(),
            provider: WeakNativeProvider {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(NativeNamespaceInner {
                nodes: Default::default(),
            })),
        };

        inner.namespaces.insert(name, namespace.clone());

        Ok(Arc::new(namespace))
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, str::FromStr};

    use support::{
        fs::in_memory::{InMemoryFile, InMemoryFileSystem},
        process::fake::FakeProcessManager,
    };

    use super::*;

    #[test]
    fn capabilities_should_return_provider_capabilities() {
        let fs = InMemoryFileSystem::default();
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs, pm);

        let capabilities = provider.capabilities();

        assert_eq!(
            capabilities,
            &ProviderCapabilities {
                requires_image: false,
                has_resources: false,
            }
        );
    }

    #[tokio::test]
    async fn tmp_dir_should_set_the_temporary_for_provider() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (
                OsString::from_str("/someotherdir").unwrap(),
                InMemoryFile::dir(),
            ),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/someotherdir");

        // we create a namespace to ensure tmp dir will be used to store namespace
        let namespace = provider.create_namespace().await.unwrap();

        assert!(namespace.base_dir().starts_with("/someotherdir"))
    }

    #[tokio::test]
    async fn create_namespace_should_create_a_new_namespace_and_returns_it() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");

        let namespace = provider.create_namespace().await.unwrap();

        // ensure namespace directory is created
        assert!(fs
            .files
            .read()
            .await
            .contains_key(namespace.base_dir().as_os_str()));

        // ensure namespace is added to provider namespaces
        assert_eq!(provider.namespaces().await.len(), 1);

        // ensure the only provider namespace is the same one as the one we just created
        assert!(provider.namespaces().await.get(namespace.name()).is_some());
    }

    #[tokio::test]
    async fn namespaces_should_return_empty_namespaces_map_if_the_provider_has_no_namespaces() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");

        assert_eq!(provider.namespaces().await.len(), 0);
    }

    #[tokio::test]
    async fn namespaces_should_return_filled_namespaces_map_if_the_provider_has_one_namespace() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");

        let namespace = provider.create_namespace().await.unwrap();

        assert_eq!(provider.namespaces().await.len(), 1);
        assert!(provider.namespaces().await.get(namespace.name()).is_some());
    }

    #[tokio::test]
    async fn namespaces_should_return_filled_namespaces_map_if_the_provider_has_two_namespaces() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm).tmp_dir("/tmp");

        let namespace1 = provider.create_namespace().await.unwrap();
        let namespace2 = provider.create_namespace().await.unwrap();

        assert_eq!(provider.namespaces().await.len(), 2);
        assert!(provider.namespaces().await.get(namespace1.name()).is_some());
        assert!(provider.namespaces().await.get(namespace2.name()).is_some());
    }
}
