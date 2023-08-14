use std::{ffi::OsString, path::Path};

use async_trait::async_trait;

mod local_file;
#[cfg(test)]
pub mod mock;

#[derive(Debug, thiserror::Error)]
pub enum FileSystemError {
    #[error("File path '{0:?}' doesn't contains UTF-8")]
    InvalidUtf8Path(OsString),
    #[error("File '{0}' doesn't contains UTF-8")]
    InvalidUtf8File(String),
    #[error("File '{0}' already exists")]
    FileAlreadyExists(String),
    #[error("File '{0}' not found")]
    FileNotFound(String),
    #[error("File '{0}' is a directory")]
    FileIsDirectory(String),
}

pub type FileSystemResult<T> = Result<T, FileSystemError>;

#[async_trait]
pub trait FileSystem {
    async fn copy(
        &self,
        from: impl AsRef<Path> + Send,
        to: impl AsRef<Path> + Send,
    ) -> FileSystemResult<()>;

    async fn create_dir(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()>;

    async fn create_dir_all(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()>;

    async fn read(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<Option<Vec<u8>>>;

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<String>;

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        content: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()>;
}
