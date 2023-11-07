use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use futures::{future::try_join_all, try_join};
use k8s_openapi::api::core::v1::PodSpec;
use support::fs::FileSystem;
use tokio::sync::{mpsc, RwLock};

use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_SCRIPTS_DIR},
    shared::helpers::{create_log_writing_task, create_stream_polling_task},
    types::{GenerateFilesOptions, SpawnNodeOptions},
    DynNode, ProviderError, ProviderNamespace,
};

use super::{
    client::KubernetesClient,
    node::{KubernetesNode, KubernetesNodeInner},
    provider::WeakKubernetesProvider,
};

#[derive(Clone)]
pub(super) struct KubernetesNamespace<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) name: String,
    pub(super) base_dir: PathBuf,
    pub(super) inner: Arc<RwLock<KubernetesNamespaceInner<FS, KC>>>,
    pub(super) filesystem: FS,
    pub(super) client: KC,
    pub(super) provider: WeakKubernetesProvider<FS, KC>,
}

#[derive(Clone)]
pub(super) struct KubernetesNamespaceInner<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) nodes: HashMap<String, KubernetesNode<FS, KC>>,
}

#[derive(Clone)]
pub(super) struct WeakKubernetesNamespace<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub inner: Weak<RwLock<KubernetesNamespaceInner<FS, KC>>>,
}

#[async_trait]
impl<FS, KC> ProviderNamespace for KubernetesNamespace<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    KC: KubernetesClient<FS> + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    async fn nodes(&self) -> HashMap<String, DynNode> {
        self.inner
            .read()
            .await
            .nodes
            .clone()
            .into_iter()
            .map(|(id, node)| (id, Arc::new(node) as DynNode))
            .collect()
    }

    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        let mut inner = self.inner.write().await;

        if inner.nodes.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name));
        }

        // create node directories and filepaths
        // TODO: remove duplication between providers
        let base_dir_raw = format!("{}/{}", &self.base_dir.to_string_lossy(), &options.name);
        let base_dir = PathBuf::from(&base_dir_raw);
        let log_path = PathBuf::from(format!("{}/{}.log", base_dir_raw, &options.name));
        let config_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_SCRIPTS_DIR));
        // NOTE: in native this base path already exist
        self.filesystem.create_dir_all(&base_dir).await?;
        try_join!(
            self.filesystem.create_dir(&config_dir),
            self.filesystem.create_dir(&data_dir),
            self.filesystem.create_dir(&scripts_dir),
        )?;

        // creat k8s pod
        let manifest = self
            .client
            .create_pod(
                &self.name,
                &options.name,
                PodSpec::default(),
                BTreeMap::from([("foo".to_string(), "bar".to_string())]),
            )
            .await
            .unwrap();

        // store pod manifest
        self.filesystem
            .write(
                PathBuf::from_iter([&base_dir, &PathBuf::from("pod_manifest.yaml")]),
                serde_yaml::to_string(&manifest).unwrap(),
            )
            .await?;

        // create paths
        let ops_fut: Vec<_> = options
            .created_paths
            .into_iter()
            .map(|created_path| {
                self.client.pod_exec(
                    &self.name,
                    &options.name,
                    vec!["mkdir", "-p", &created_path.to_string_lossy()]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect(),
                )
            })
            .collect();
        // TODO: handle the error conversion correctly
        try_join_all(ops_fut).await.unwrap();

        // copy injected files
        let ops_fut: Vec<_> = options
            .injected_files
            .iter()
            .map(|file| {
                self.client.copy_to_pod(
                    &self.name,
                    &options.name,
                    &file.local_path,
                    &file.remote_path,
                    &file.mode,
                )
            })
            .collect();
        // TODO: handle the error conversion correctly
        try_join_all(ops_fut).await.unwrap();

        // start process
        // TODO: handle the error conversion correctly
        self.client
            .pod_exec(
                &self.name,
                &options.name,
                vec!["echo", "start", ">", "/tmp/zombiepipe"],
            )
            .await
            .unwrap()
            .unwrap();

        // create log stream
        let logs_stream = self
            .client
            .create_pod_logs_stream(&options.name, &self.name)
            .await
            .unwrap();

        // handle log writing
        let (logs_tx, rx) = mpsc::channel(10);
        let log_reading_handle = create_stream_polling_task(logs_stream, logs_tx);
        let log_writing_handle =
            create_log_writing_task(rx, self.filesystem.clone(), log_path.clone());

        // create node structure holding state
        let node = KubernetesNode {
            name: options.name.clone(),
            program: options.program,
            args: options.args,
            env: options.env,
            base_dir,
            config_dir,
            data_dir,
            scripts_dir,
            log_path,
            filesystem: self.filesystem.clone(),
            client: self.client.clone(),
            namespace: WeakKubernetesNamespace {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(KubernetesNodeInner {
                log_reading_handle,
                log_writing_handle,
            })),
        };

        // store node inside namespace
        inner.nodes.insert(options.name, node.clone());

        Ok(Arc::new(node))
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        todo!()
    }

    async fn static_setup(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        todo!()
    }
}
