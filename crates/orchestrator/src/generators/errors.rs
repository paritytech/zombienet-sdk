use provider::ProviderError;
use support::fs::FileSystemError;

#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("Generating key {0} with input {1}")]
    KeyGeneration(String, String),
    #[error("Generating port {0}, err {1}")]
    PortGeneration(u16, String),
    #[error("Chain-spec build error: {0}")]
    ChainSpecGeneration(String),
    #[error("Provider error: {0}")]
    ProviderError(#[from] ProviderError),
    #[error("FileSystem error")]
    FileSystemError(#[from] FileSystemError),
    #[error("Generating identity, err {0}")]
    IdentityGeneration(String),
    #[error("Generating bootnode address, err {0}")]
    BootnodeAddrGeneration(String),
}
