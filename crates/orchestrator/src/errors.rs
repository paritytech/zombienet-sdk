//! Zombienet Orchestrator error definitions.

use provider::ProviderError;
use support::fs::FileSystemError;

use crate::generators;

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    // TODO: improve invalid config reporting
    #[error("Invalid network configuration: {0}")]
    InvalidConfig(String),
    #[error("Invalid network config to use provider {0}: {1}")]
    InvalidConfigForProvider(String, String),
    #[error("Invalid configuration for node: {0}, field: {1}")]
    InvalidNodeConfig(String, String),
    #[error("Invariant not fulfilled {0}")]
    InvariantError(&'static str),
    #[error("Global network spawn timeout: {0} secs")]
    GlobalTimeOut(u32),
    #[error("Generator error: {0}")]
    GeneratorError(#[from] generators::errors::GeneratorError),
    #[error("Provider error")]
    ProviderError(#[from] ProviderError),
    #[error("FileSystem error")]
    FileSystemError(#[from] FileSystemError),
    #[error(transparent)]
    SpawnerError(#[from] anyhow::Error),
}
