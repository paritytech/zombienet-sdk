pub mod native;
pub mod shared;

use std::{
    collections::HashMap, net::IpAddr, path::PathBuf, process::ExitStatus, sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use shared::types::{
    GenerateFilesOptions, ProviderCapabilities, RunCommandOptions, RunScriptOptions,
    SpawnNodeOptions,
};
use support::fs::FileSystemError;

use crate::shared::types::Port;

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ProviderError {
    #[error("Failed to spawn node '{0}': {1}")]
    NodeSpawningFailed(String, anyhow::Error),

    #[error("Error running command '{0}': {1}")]
    RunCommandError(String, anyhow::Error),

    #[error("Invalid network configuration field {0}")]
    InvalidConfig(String),

    #[error("Can recover node: {0} info, field: {1}")]
    MissingNodeInfo(String, String),

    #[error("Duplicated node name: {0}")]
    DuplicatedNodeName(String),

    #[error("File generation failed: {0}")]
    FileGenerationFailed(anyhow::Error),

    #[error(transparent)]
    FileSystemError(#[from] FileSystemError),

    #[error("Invalid script path for {0}")]
    InvalidScriptPath(anyhow::Error),

    #[error("Script with path {0} not found")]
    ScriptNotFound(PathBuf),

    #[error("Failed to retrieve process ID for node '{0}'")]
    ProcessIdRetrievalFailed(String),

    #[error("Failed to pause node '{0}'")]
    PauseNodeFailed(String),

    #[error("Failed to resume node '{0}'")]
    ResumeNodeFaied(String),

    #[error("Failed to kill node '{0}'")]
    KillNodeFailed(String),
}

#[async_trait]
pub trait Provider {
    fn capabilities(&self) -> &ProviderCapabilities;

    async fn namespaces(&self) -> HashMap<String, DynNamespace>;

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError>;
}

pub type DynProvider = Arc<dyn Provider>;

#[async_trait]
pub trait ProviderNamespace {
    fn name(&self) -> &str;

    fn base_dir(&self) -> &PathBuf;

    async fn nodes(&self) -> HashMap<String, DynNode>;

    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError>;

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError>;

    async fn destroy(&self) -> Result<(), ProviderError>;

    async fn static_setup(&self) -> Result<(), ProviderError>;
}

pub type DynNamespace = Arc<dyn ProviderNamespace + Send + Sync>;

type ExecutionResult = Result<String, (ExitStatus, String)>;

#[async_trait]
pub trait ProviderNode {
    fn name(&self) -> &str;

    fn program(&self) -> &str;

    fn args(&self) -> Vec<&str>;

    async fn ip(&self) -> Result<IpAddr, ProviderError>;

    fn base_dir(&self) -> &PathBuf;

    fn config_dir(&self) -> &PathBuf;

    fn data_dir(&self) -> &PathBuf;

    fn scripts_dir(&self) -> &PathBuf;

    fn log_path(&self) -> &PathBuf;

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

// re-export
pub use native::NativeProvider;
pub use shared::{constants, types};
