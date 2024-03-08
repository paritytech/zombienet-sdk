mod kubernetes;
mod native;
pub mod shared;

use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use shared::{
    constants::LOCALHOST,
    types::{
        ExecutionResult, GenerateFilesOptions, ProviderCapabilities, RunCommandOptions,
        RunScriptOptions, SpawnNodeOptions,
    },
};
use support::fs::FileSystemError;

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ProviderError {
    #[error("Failed to create client '{0}': {1}")]
    CreateClientFailed(String, anyhow::Error),

    #[error("Failed to create namespace '{0}': {1}")]
    CreateNamespaceFailed(String, anyhow::Error),

    #[error("Failed to spawn node '{0}': {1}")]
    NodeSpawningFailed(String, anyhow::Error),

    #[error("Error running command '{0}' in pod {1}: {2}")]
    RunCommandError(String, String, anyhow::Error),

    #[error("Error running script'{0}': {1}")]
    RunScriptError(String, anyhow::Error),

    #[error("Invalid network configuration field {0}")]
    InvalidConfig(String),

    #[error("Can not recover node: {0}")]
    MissingNode(String),

    #[error("Can not recover node: {0} info, field: {1}")]
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

    #[error("Failed to pause node '{0}': {1}")]
    PauseNodeFailed(String, anyhow::Error),

    #[error("Failed to resume node '{0}': {1}")]
    ResumeNodeFailed(String, anyhow::Error),

    #[error("Failed to kill node '{0}': {1}")]
    KillNodeFailed(String, anyhow::Error),

    #[error("Failed to restart node '{0}': {1}")]
    RestartNodeFailed(String, anyhow::Error),

    #[error("Failed to destroy node '{0}': {1}")]
    DestroyNodeFailed(String, anyhow::Error),

    #[error("Failed to get logs for node '{0}': {1}")]
    GetLogsFailed(String, anyhow::Error),

    #[error("Failed to dump logs for node '{0}': {1}")]
    DumpLogsFailed(String, anyhow::Error),

    #[error("Failed to copy file from node '{0}': {1}")]
    CopyFileFromNodeError(String, anyhow::Error),

    #[error("Failed to setup fileserver: {0}")]
    FileServerSetupError(anyhow::Error),

    #[error("Error uploading file: '{0}': {1}")]
    UploadFile(String, anyhow::Error),

    #[error("Error downloading file: '{0}': {1}")]
    DownloadFile(String, anyhow::Error),

    #[error("Error sending file: '{0}': {1}")]
    SendFile(String, anyhow::Error),

    #[error("Error creating port-forward '{0}:{1}': {2}")]
    PortForwardError(u16, u16, anyhow::Error),

    #[error("Failed to delete namespace '{0}': {1}")]
    DeleteNamespaceFailed(String, anyhow::Error),
}

#[async_trait]
pub trait Provider {
    fn name(&self) -> &str;

    fn capabilities(&self) -> &ProviderCapabilities;

    async fn namespaces(&self) -> HashMap<String, DynNamespace>;

    async fn create_namespace(&self) -> Result<DynNamespace, ProviderError>;
}

pub type DynProvider = Arc<dyn Provider + Send + Sync>;

#[async_trait]
pub trait ProviderNamespace {
    fn name(&self) -> &str;

    fn base_dir(&self) -> &PathBuf;

    fn capabilities(&self) -> &ProviderCapabilities;

    async fn detach(&self) {
        // noop by default
        warn!("Detach is not implemented for {}", self.name());
    }

    async fn nodes(&self) -> HashMap<String, DynNode>;

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError>;

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError>;

    async fn destroy(&self) -> Result<(), ProviderError>;

    async fn static_setup(&self) -> Result<(), ProviderError>;
}

pub type DynNamespace = Arc<dyn ProviderNamespace + Send + Sync>;

#[async_trait]
pub trait ProviderNode {
    fn name(&self) -> &str;

    fn args(&self) -> Vec<&str>;

    fn base_dir(&self) -> &PathBuf;

    fn config_dir(&self) -> &PathBuf;

    fn data_dir(&self) -> &PathBuf;

    fn relay_data_dir(&self) -> &PathBuf;

    fn scripts_dir(&self) -> &PathBuf;

    fn log_path(&self) -> &PathBuf;

    // Return the absolute path to the file in the `node` perspective
    // TODO: purpose?
    fn path_in_node(&self, file: &Path) -> PathBuf;

    async fn logs(&self) -> Result<String, ProviderError>;

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError>;

    // By default return localhost, should be overrided for k8s
    async fn ip(&self) -> Result<IpAddr, ProviderError> {
        Ok(LOCALHOST)
    }

    // Noop by default (native provider)
    async fn create_port_forward(
        &self,
        _local_port: u16,
        _remote_port: u16,
    ) -> Result<Option<u16>, ProviderError> {
        Ok(None)
    }

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError>;

    async fn run_script(&self, options: RunScriptOptions)
        -> Result<ExecutionResult, ProviderError>;

    async fn send_file(
        &self,
        local_file_path: &Path,
        remote_file_path: &Path,
        mode: &str,
    ) -> Result<(), ProviderError>;

    async fn receive_file(
        &self,
        remote_file_path: &Path,
        local_file_path: &Path,
    ) -> Result<(), ProviderError>;

    async fn pause(&self) -> Result<(), ProviderError>;

    async fn resume(&self) -> Result<(), ProviderError>;

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError>;

    async fn destroy(&self) -> Result<(), ProviderError>;
}

pub type DynNode = Arc<dyn ProviderNode + Send + Sync>;

// re-export
pub use kubernetes::*;
pub use native::*;
pub use shared::{constants, types};
use tracing::warn;
