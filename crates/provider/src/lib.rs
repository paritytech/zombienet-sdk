mod native;
mod shared;

use std::{net::IpAddr, path::PathBuf, process::ExitStatus, sync::Arc, time::Duration};

use async_trait::async_trait;

use crate::shared::types::{FileMap, Port};

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

macro_rules! common_options {
    () => {
        fn args(mut self, args: Vec<String>) -> Self {
            self.args = args;
            self
        }

        fn env(mut self, env: Vec<(String, String)>) -> Self {
            self.env = env;
            self
        }
    };
}

pub struct SpawnNodeOptions {
    name: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl SpawnNodeOptions {
    fn new(name: String, command: String) -> Self {
        Self {
            name,
            command,
            args: vec![],
            env: vec![],
        }
    }

    common_options!();
}

pub struct SpawnTempOptions {
    pub node: (),
    pub injected_files: Vec<FileMap>,
    pub files_to_retrieve: Vec<FileMap>,
}

#[async_trait]
pub trait ProviderNamespace {
    async fn id(&self) -> String;
    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError>;
    async fn spawn_temp(&self, options: SpawnTempOptions) -> Result<(), ProviderError>;
    async fn destroy(&self) -> Result<(), ProviderError>;
    async fn static_setup(&self) -> Result<(), ProviderError>;
}

pub type DynNamespace = Arc<dyn ProviderNamespace>;

pub struct RunCommandOptions {
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

impl RunCommandOptions {
    fn new(command: String) -> Self {
        Self {
            command,
            args: vec![],
            env: vec![],
        }
    }

    common_options!();
}

pub struct RunScriptOptions {
    pub(crate) local_script_path: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

impl RunScriptOptions {
    fn new(local_script_path: String) -> Self {
        Self {
            local_script_path,
            args: vec![],
            env: vec![],
        }
    }

    common_options!();
}

type ExecutionResult = Result<String, (ExitStatus, String)>;

#[async_trait]
pub trait ProviderNode {
    async fn name(&self) -> String;

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
