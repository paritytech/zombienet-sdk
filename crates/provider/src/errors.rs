//! Zombienet Provider error definitions.

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ProviderError {
    #[error("Invalid network configuration")]
    InvalidConfig,
    #[error("TODO")]
    TodoErr,
}