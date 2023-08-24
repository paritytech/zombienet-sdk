//! Zombienet Orchestrator error definitions.

use crate::generators;

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    // TODO: improve invalid config reporting
    #[error("Invalid network configuration: {0}")]
    InvalidConfig(String),
    #[error("Invalid configuration for node: {0}, field: {1}")]
    InvalidNodeConfig(String, String),
    #[error("Global network spawn timeout: {0} secs")]
    GlobalTimeOut(u32),
    #[error("Generator error")]
    GeneratorError(#[from] generators::errors::GeneratorError),
}
