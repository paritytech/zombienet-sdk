//! Zombienet Provider error definitions.

#[derive(Debug, thiserror::Error)]
pub enum FileSystemError {
    // TODO: we need more specifc error
    #[error("Generic FileSystem error")]
    GenericFileSystemError,
    /// Some other error.
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Sync + Send + 'static>),
}
