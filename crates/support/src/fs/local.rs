use std::path::Path;
use tokio::io::AsyncWriteExt;

use async_trait::async_trait;

use super::{FileSystem, FileSystemError, FileSystemResult};

pub struct LocalFileSystem;

#[async_trait]
impl FileSystem for LocalFileSystem {
    async fn create_dir(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()> {
        tokio::fs::create_dir(path).await.map_err(Into::into)
    }

    async fn create_dir_all(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()> {
        tokio::fs::create_dir_all(path).await.map_err(Into::into)
    }

    async fn read(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<Vec<u8>> {
        tokio::fs::read(path).await.map_err(Into::into)
    }

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<String> {
        tokio::fs::read_to_string(path).await.map_err(Into::into)
    }

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()> {
        tokio::fs::write(path, contents).await.map_err(Into::into)
    }

    async fn append(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()> {
        let contents = contents.as_ref();
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .await
            .map_err(Into::<FileSystemError>::into)?;

        file.write_all(contents)
            .await
            .and(Ok(()))
            .map_err(Into::into)
    }

    async fn copy(
        &self,
        from: impl AsRef<Path> + Send,
        to: impl AsRef<Path> + Send,
    ) -> FileSystemResult<()> {
        tokio::fs::copy(from, to)
            .await
            .and(Ok(()))
            .map_err(Into::into)
    }
}
