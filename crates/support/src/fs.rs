use std::io::Write;

use async_trait::async_trait;

mod local_file;
pub mod mock;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[async_trait]
pub trait FileSystem {
    type LocalFile: Write;

    fn copy(&mut self, from: impl Into<String>, to: impl Into<String>) -> Result<()>;
    fn create(&mut self, path: impl Into<String>) -> Result<Self::LocalFile>;
    fn create_dir(&mut self, path: impl Into<String>) -> Result<()>;
    fn open_file(&mut self, path: impl Into<String>) -> Result<()>;
    fn read_file(&mut self, path: impl Into<String>) -> Result<String>;
    fn write(&mut self, path: impl Into<String>, content: impl Into<String>) -> Result<()>;
}

#[derive(Debug)]
struct FilesystemInMemory {}
