use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;

use super::namespace::NativeNamespace;
use crate::{
    types::ProviderCapabilities, DynNamespace, Provider, ProviderError, ProviderNamespace,
};

const PROVIDER_NAME: &str = "native";

pub struct NativeProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<NativeProvider<FS>>,
    capabilities: ProviderCapabilities,
    tmp_dir: PathBuf,
    filesystem: FS,
    pub(super) namespaces: RwLock<HashMap<String, Arc<NativeNamespace<FS>>>>,
}

impl<FS> NativeProvider<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    pub fn new(filesystem: FS) -> Arc<Self> {
        Arc::new_cyclic(|weak| NativeProvider {
            weak: weak.clone(),
            capabilities: ProviderCapabilities {
                has_resources: false,
                requires_image: false,
                prefix_with_full_path: true,
                use_default_ports_in_cmd: false,
            },
            // NOTE: temp_dir in linux return `/tmp` but on mac something like
            //  `/var/folders/rz/1cyx7hfj31qgb98d8_cg7jwh0000gn/T/`, having
            // one `trailing slash` and the other no can cause issues if
            // you try to build a fullpath by concatenate. Use Pathbuf to prevent the issue.
            tmp_dir: std::env::temp_dir(),
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
impl<FS> Provider for NativeProvider<FS>
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
        let namespace = NativeNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
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
        let namespace = NativeNamespace::new(
            &self.weak,
            &self.tmp_dir,
            &self.capabilities,
            &self.filesystem,
            Some(&base_dir),
        )
        .await?;

        self.namespaces
            .write()
            .await
            .insert(namespace.name().to_string(), namespace.clone());

        Ok(namespace)
    }
}
