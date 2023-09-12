mod native;
mod shared;

use std::{net::IpAddr, path::PathBuf, process::ExitStatus, sync::Arc, time::Duration};

use async_trait::async_trait;
use shared::types::TransferedFile;

use crate::shared::types::Port;

use support::fs::FileSystemError;

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ProviderError {
    #[error("Failed to spawn node '{0}': {1}")]
    NodeSpawningFailed(String, anyhow::Error),

    #[error("Error running command: {0}")]
    RunCommandError(anyhow::Error),

    #[error("Invalid network configuration field {0}")]
    InvalidConfig(String),

    #[error("Can recover node: {0} info, field: {1}")]
    MissingNodeInfo(String, String),

    #[error("Duplicated node name: {0}")]
    DuplicatedNodeName(String),

    #[error(transparent)]
    FSError(#[from] FileSystemError),

    #[error("Invalid script path for {0}")]
    InvalidScriptPath(String),

    #[error("File generation failed: {0}")]
    FileGenerationFailed(anyhow::Error),
}

#[derive(Debug, Clone)]
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

#[async_trait]
pub trait Provider {
    fn capabilities(&self) -> ProviderCapabilities;
    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError>;
    // TODO(team): Do we need at this point to handle cleanner/pod-monitor?
}

pub type DynProvider = Arc<dyn Provider>;

pub struct SpawnNodeOptions {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub injected_files: Vec<TransferedFile>,
}

pub struct GenerateFileCommand {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub local_output_path: String,
}

pub struct GenerateFilesOptions {
    pub commands: Vec<GenerateFileCommand>,
    pub injected_files: Vec<TransferedFile>,
}

#[async_trait]
pub trait ProviderNamespace {
    fn id(&self) -> String;

    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError>;

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError>;

    async fn destroy(&self) -> Result<(), ProviderError>;

    async fn static_setup(&self) -> Result<(), ProviderError>;
}

pub type DynNamespace = Arc<dyn ProviderNamespace>;

pub struct RunCommandOptions {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub struct RunScriptOptions {
    pub local_script_path: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

type ExecutionResult = Result<String, (ExitStatus, String)>;

#[async_trait]
pub trait ProviderNode {
    fn name(&self) -> String;

    async fn endpoint(&self) -> Result<(IpAddr, Port), ProviderError>;

    async fn mapped_port(&self, port: Port) -> Result<Port, ProviderError>;

    async fn logs(&self) -> Result<String, ProviderError>;

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError>;

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

    async fn pause(&self) -> Result<(), ProviderError>;

    async fn resume(&self) -> Result<(), ProviderError>;

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError>;

    async fn destroy(&self) -> Result<(), ProviderError>;
}

pub type DynNode = Arc<dyn ProviderNode + Send + Sync>;
