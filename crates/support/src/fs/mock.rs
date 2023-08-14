use std::{collections::HashMap, path::Path, ffi::OsString};

use super::{FileSystem, FileSystemError, FileSystemResult};
use async_trait::async_trait;
use tokio::sync::RwLock;

enum InMemoryFileType {
    File,
    Directory,
}

struct InMemoryFile {
    r#type: InMemoryFileType,
    content: Option<Vec<u8>>,
}

impl InMemoryFile {
    fn dir() -> Self {
        Self {
            r#type: InMemoryFileType::Directory,
            content: None,
        }
    }

    fn file(content: Option<Vec<u8>>) -> Self {
        Self {
            r#type: InMemoryFileType::File,
            content,
        }
    }
}

struct InMemoryFileSystem {
    files: RwLock<HashMap<OsString, InMemoryFile>>,
}

#[async_trait]
impl FileSystem for InMemoryFileSystem {
    async fn copy(
        &self,
        from: impl AsRef<Path> + Send,
        to: impl AsRef<Path> + Send,
    ) -> FileSystemResult<()> {
        let from = from.as_ref().to_owned();
        let to = to.as_ref().to_owned();

        from.as_os_str()
    }

    async fn create_dir(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()> {}

    async fn create_dir_all(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()> {
        let ancestors = path.as_ref().to_owned().ancestors();
        let files = self.files.write().await;

        while let Some(path) = ancestors.next() {
            let path = path
                .to_str()
                .ok_or(FileSystemError::InvalidUtf8Path(
                    path.as_os_str().to_owned(),
                ))?
                .to_string();

            if files.contains_key(&path) {
                return Err(FileSystemError::FileAlreadyExists(path.clone()));
            }

            files.insert(path, InMemoryFile::dir());
        }

        Ok(())
    }

    async fn read(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<Option<Vec<u8>>> {
        let path = path.as_ref().to_owned();
        let path = path
            .to_str()
            .ok_or(FileSystemError::InvalidUtf8Path(
                path.as_os_str().to_owned(),
            ))?
            .to_string();
        let file = self
            .files
            .read()
            .await
            .get(&path)
            .ok_or(FileSystemError::FileNotFound(path))?;

        if let InMemoryFileType::Directory = file.r#type {
            return Err(FileSystemError::FileIsDirectory(path));
        }

        Ok(file.content.clone())
    }

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<String> {
        let content = self.read(path).await?;
        let path = path.as_ref().to_str().unwrap().to_string();

        Ok(match content {
            Some(content) => {
                String::from_utf8(content).map_err(|_| FileSystemError::InvalidUtf8File(path))?
            },
            None => String::from(""),
        })
    }

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()> {
        let files = self.files.write().await;

        if !files.contains_key(&path) {
            return Err(FileSystemError::FileNotFound(path));
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn it_works() {}
}

// #[derive(Debug, PartialEq)]
// pub enum Operation {
//     Copy { from: PathBuf, to: PathBuf },
//     ReadFile { path: PathBuf },
//     CreateFile { path: PathBuf },
//     CreateDir { path: PathBuf },
//     OpenFile { path: PathBuf },
//     Write { path: PathBuf, content: String },
// }

// #[derive(Debug, thiserror::Error)]
// pub enum MockError {
//     #[error("Operation error: {0}")]
//     OpError(String),
//     #[error(transparent)]
//     Other(#[from] Box<dyn std::error::Error + Sync + Send + 'static>),
// }
// #[derive(Debug, Default)]
// pub struct MockFilesystem {
//     copy_error: Option<MockError>,
//     create_dir_error: Option<MockError>,
//     create_file_error: Option<MockError>,
//     open_file_error: Option<MockError>,
//     read_file_error: Option<MockError>,
//     write_error: Option<MockError>,
//     pub operations: Vec<Operation>,
// }

// impl MockFilesystem {
//     pub fn new() -> Self {
//         Self::default()
//     }

//     pub fn with_create_dir_error(error: MockError) -> Self {
//         Self {
//             create_dir_error: Some(error),
//             ..Self::default()
//         }
//     }

//     // TODO: add test
//     #[allow(dead_code)]
//     fn with_create_file_error(error: MockError) -> Self {
//         Self {
//             create_file_error: Some(error),
//             ..Self::default()
//         }
//     }

//     // TODO: add test
//     #[allow(dead_code)]
//     fn with_read_file_error(error: MockError) -> Self {
//         Self {
//             read_file_error: Some(error),
//             ..Self::default()
//         }
//     }

//     // TODO: add test
//     #[allow(dead_code)]
//     fn with_copy_error(error: MockError) -> Self {
//         Self {
//             copy_error: Some(error),
//             ..Self::default()
//         }
//     }

//     // TODO: add test
//     #[allow(dead_code)]
//     fn with_write_error(error: MockError) -> Self {
//         Self {
//             write_error: Some(error),
//             ..Self::default()
//         }
//     }
// }

// #[async_trait]
// impl FileSystem for MockFilesystem {
//     type FSError = MockError;
//     type File = LocalFile;

//     async fn create_dir<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<(), Self::FSError> {
//         if let Some(err) = self.create_dir_error.take() {
//             return Err(err);
//         }

//         self.operations.push(Operation::CreateDir {
//             path: path.as_ref().to_path_buf(),
//         });
//         Ok(())
//     }

//     async fn write<P: AsRef<Path> + Send>(
//         &mut self,
//         path: P,
//         content: impl Into<String> + Send,
//     ) -> Result<(), Self::FSError> {
//         if let Some(err) = self.write_error.take() {
//             return Err(err);
//         }

//         self.operations.push(Operation::Write {
//             path: path.as_ref().to_path_buf(),
//             content: content.into(),
//         });
//         Ok(())
//     }

//     async fn create<P: AsRef<Path> + Send>(
//         &mut self,
//         path: P,
//     ) -> Result<Self::File, Self::FSError> {
//         if let Some(err) = self.create_file_error.take() {
//             return Err(err);
//         }

//         let p = path.as_ref().to_path_buf();

//         self.operations
//             .push(Operation::CreateFile { path: p.clone() });

//         let file = File::create(p).expect("not created");
//         Ok(LocalFile::from(file))
//     }

//     async fn open_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<(), Self::FSError> {
//         if let Some(err) = self.open_file_error.take() {
//             return Err(err);
//         }

//         self.operations.push(Operation::OpenFile {
//             path: path.as_ref().to_path_buf(),
//         });
//         Ok(())
//     }

//     async fn read_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<String, Self::FSError> {
//         if let Some(err) = self.read_file_error.take() {
//             return Err(err);
//         }

//         self.operations.push(Operation::ReadFile {
//             path: path.as_ref().to_path_buf(),
//         });
//         Ok("This is a test".to_owned())
//     }

//     async fn copy<P: AsRef<Path> + Send>(
//         &mut self,
//         from: P,
//         to: P,
//     ) -> std::result::Result<(), Self::FSError> {
//         if let Some(err) = self.copy_error.take() {
//             return Err(err);
//         }

//         self.operations.push(Operation::Copy {
//             from: from.as_ref().to_path_buf(),
//             to: to.as_ref().to_path_buf(),
//         });
//         Ok(())
//     }
// }
