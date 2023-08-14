mod errors;
mod native;
mod shared;

use std::{net::IpAddr, path::PathBuf, process::ExitStatus, sync::Arc};

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::{
    errors::ProviderError,
    shared::types::{FileMap, Port},
};

#[derive(Clone)]
pub struct ProviderCapabilities {
    pub requires_image: bool,
}

pub struct CreateNamespaceOptions {
    pub root_dir: String,
    pub config_dir: String,
    pub data_dir: String,
}

impl Default for CreateNamespaceOptions {
    fn default() -> Self {
        Self {
            root_dir: "/tmp".to_string(),
            config_dir: "/cfg".to_string(),
            data_dir: "/data".to_string(),
        }
    }
}

impl CreateNamespaceOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root_dir(mut self, root_dir: &str) -> Self {
        self.root_dir = root_dir.to_string();
        self
    }

    pub fn config_dir(mut self, config_dir: &str) -> Self {
        self.config_dir = config_dir.to_string();
        self
    }

    pub fn data_dir(mut self, data_dir: &str) -> Self {
        self.data_dir = data_dir.to_string();
        self
    }
}

#[async_trait]
pub trait Provider {
    fn capabilities(&self) -> &ProviderCapabilities;
    async fn create_namespace(
        &self,
        options: Option<CreateNamespaceOptions>,
    ) -> Result<DynNamespace, ProviderError>;
    // TODO(team): Do we need at this point to handle cleanner/pod-monitor?
}

pub type DynProvider = Arc<RwLock<dyn Provider>>;

pub struct SpawnNodeOptions {
    pub name: String,
    pub node: (),
    // Files to inject, `before` we run the provider command.
    pub files_inject: Vec<FileMap>,
    // TODO: keystore logic should live in the orchestrator
    pub keystore: String,
    // chain_spec_id: String,
    // TODO: abstract logic for download and uncompress
    pub db_snapshot: String,
}

pub struct SpawnTempOptions {
    pub node: (),
    pub injected_files: Vec<FileMap>,
    pub files_to_retrieve: Vec<FileMap>,
}

#[async_trait]
pub trait ProviderNamespace {
    fn id(&self) -> &str;
    /// Spawn a long live node/process.
    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<(), ProviderError>;
    /// Spawn a temporary node, will be shutdown after `get` the desired files or output.
    async fn spawn_temp(&self, options: SpawnTempOptions) -> Result<(), ProviderError>;
    /// Destroy namespace (and inner resources).
    async fn destroy(&self) -> Result<(), ProviderError>;
    async fn static_setup(&self) -> Result<(), ProviderError>;
}

pub type DynNamespace = Arc<RwLock<dyn ProviderNamespace>>;

pub struct RunCommandOptions {
    pub args: Vec<String>,
    pub is_failure_allowed: bool,
}

pub struct RunScriptOptions {
    pub identifier: String,
    pub script_path: String,
    pub args: Vec<String>,
}

type ExecutionResult = Result<String, (ExitStatus, Option<String>)>;

#[async_trait]
pub trait ProviderNode {
    fn name(&self) -> &str;

    async fn endpoint(&self) -> Result<(IpAddr, Port), ProviderError>;

    async fn mapped_port(&self, port: Port) -> Result<Port, ProviderError>;

    async fn logs(&self) -> Result<String, ProviderError>;

    async fn dump_logs(&self, dest: PathBuf) -> Result<(), ProviderError>;

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError>;

    async fn run_script(&self, options: RunScriptOptions)
        -> Result<ExecutionResult, ProviderError>;

    async fn copy_file_from_node(
        &self,
        remote_src: PathBuf,
        local_dest: PathBuf,
    ) -> Result<(), ProviderError>;

    async fn pause(&self, node_name: &str) -> Result<(), ProviderError>;

    async fn resume(&self, node_name: &str) -> Result<(), ProviderError>;

    async fn restart(
        &mut self,
        node_name: &str,
        after_sec: Option<u16>,
    ) -> Result<bool, ProviderError>;

    async fn destroy(&self) -> Result<(), ProviderError>;
}

pub type DynNode = Arc<RwLock<dyn ProviderNode>>;
