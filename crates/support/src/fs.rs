use std::{
    io::{Read, Write},
    path::Path,
    process::Stdio,
};

use async_trait::async_trait;

pub mod errors;
mod local_file;
pub mod mock;

#[async_trait]
pub trait FileSystem {
    type File: Read + Write + Into<Stdio> + Send + Sync;
    type FSError: std::error::Error + Send + Sync + 'static;

    async fn copy<P: AsRef<Path> + Send>(&mut self, from: P, to: P) -> Result<(), Self::FSError>;
    async fn create<P: AsRef<Path> + Send>(&mut self, path: P)
        -> Result<Self::File, Self::FSError>;
    async fn create_dir<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<(), Self::FSError>;
    async fn open_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<(), Self::FSError>;
    async fn read_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<String, Self::FSError>;
    async fn write<P: AsRef<Path> + Send>(
        &mut self,
        path: P,
        content: impl Into<String> + Send,
    ) -> Result<(), Self::FSError>;
}

// #[derive(Debug)]
// struct FilesystemInMemory {}
