use std::{ffi::OsString, path::Path};

use async_trait::async_trait;

#[cfg(test)]
pub mod in_memory;

#[derive(Debug, thiserror::Error)]
pub enum FileSystemError {
    #[error("File {0:?} already exists")]
    FileAlreadyExists(OsString),
    #[error("Directory {0:?} already exists")]
    DirectoryAlreadyExists(OsString),
    #[error("Ancestor {0:?} doesn't exists")]
    AncestorDoesntExists(OsString),
    #[error("Ancestor {0:?} is not a directory")]
    AncestorNotDirectory(OsString),
    #[error("File {0:?} not found")]
    FileNotFound(OsString),
    #[error("File {0:?} is a directory")]
    FileIsDirectory(OsString),
    #[error("Invalid UTF-8 encoding for file {0:?}")]
    InvalidUtf8FileEncoding(OsString),
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

    async fn read(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<Vec<u8>>;

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<String>;

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        content: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()>;
}
