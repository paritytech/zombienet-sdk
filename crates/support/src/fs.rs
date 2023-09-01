use std::path::Path;

use async_trait::async_trait;

pub mod in_memory;
pub mod local;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct FileSystemError(#[from] anyhow::Error);

impl From<std::io::Error> for FileSystemError {
    fn from(error: std::io::Error) -> Self {
        Self(error.into())
    }
}

pub type FileSystemResult<T> = Result<T, FileSystemError>;

#[async_trait]
pub trait FileSystem {
    async fn create_dir(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()>;

    async fn create_dir_all(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()>;

    async fn read(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<Vec<u8>>;

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<String>;

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()>;

    async fn append(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()>;

    async fn copy(
        &self,
        from: impl AsRef<Path> + Send,
        to: impl AsRef<Path> + Send,
    ) -> FileSystemResult<()>;
}
