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
    async fn create_dir<P>(&self, path: P) -> FileSystemResult<()>
    where
        P: AsRef<Path> + Send;

    async fn create_dir_all<P>(&self, path: P) -> FileSystemResult<()>
    where
        P: AsRef<Path> + Send;

    async fn read<P>(&self, path: P) -> FileSystemResult<Vec<u8>>
    where
        P: AsRef<Path> + Send;

    async fn read_to_string<P>(&self, path: P) -> FileSystemResult<String>
    where
        P: AsRef<Path> + Send;

    async fn write<P, C>(&self, path: P, contents: C) -> FileSystemResult<()>
    where
        P: AsRef<Path> + Send,
        C: AsRef<[u8]> + Send;

    async fn append<P, C>(&self, path: P, contents: C) -> FileSystemResult<()>
    where
        P: AsRef<Path> + Send,
        C: AsRef<[u8]> + Send;

    async fn copy<P1, P2>(&self, from: P1, to: P2) -> FileSystemResult<()>
    where
        P1: AsRef<Path> + Send,
        P2: AsRef<Path> + Send;

    async fn set_mode<P>(&self, path: P, perm: u32) -> FileSystemResult<()>
    where
        P: AsRef<Path> + Send;

    async fn exists<P>(&self, path: P) -> bool
    where
        P: AsRef<Path> + Send;
}
