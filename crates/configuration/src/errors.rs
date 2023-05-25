#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ConfigError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Serialization error")]
    SerializationError,
    #[error("Unexpected rule: \n {0}")]
    Unexpected(String),
}
