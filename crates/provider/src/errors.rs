//! Zombienet Provider error definitions.

macro_rules! from_error {
    ($type:ty, $target:ident, $targetvar:expr) => {
        impl From<$type> for $target {
            fn from(s: $type) -> Self {
                $targetvar(s.into())
            }
        }
    };
}

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ProviderError {
    #[error("Namespace ID already exists: {0}")]
    ConflictingNamespaceId(String),

    #[error("Invalid network configuration field {0}")]
    InvalidConfig(String),
    #[error("Can recover node: {0} info, field: {1}")]
    MissingNodeInfo(String, String),
    #[error("Duplicated node name: {0}")]
    DuplicatedNodeName(String),
    #[error("Error running cmd: {0}")]
    RunCommandError(String),
    #[error("Error spawning node: {0}")]
    ErrorSpawningNode(String),
    #[error("Node die/stale, logs: {0}")]
    NodeNotReady(String),
    // FSErrors are implemented in the associated type
    #[error(transparent)]
    FSError(Box<dyn std::error::Error + Sync + Send + 'static>),
    // From serde errors
    #[error("Serialization error")]
    SerializationError(serde_json::Error),
    #[error("IO error: {0}")]
    IOError(std::io::Error),
    #[error("Invalid script_path: {0}")]
    InvalidScriptPath(String),
}

from_error!(
    serde_json::Error,
    ProviderError,
    ProviderError::SerializationError
);
from_error!(std::io::Error, ProviderError, ProviderError::IOError);
