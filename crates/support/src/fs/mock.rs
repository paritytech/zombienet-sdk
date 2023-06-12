use std::{error::Error, fs::File, path::{PathBuf, Path}};

use async_trait::async_trait;

use super::{local_file::LocalFile, FileSystem};

#[derive(Debug, PartialEq)]
pub enum Operation {
    // DeleteFile { path: String },
    // DeleteDir,
    // LinkFile,
    Copy { from: PathBuf, to: PathBuf },
    ReadFile { path: PathBuf },
    CreateFile { path: PathBuf },
    CreateDir { path: PathBuf },
    OpenFile { path: PathBuf },
    Write { path: PathBuf, content: String },
}

#[derive(Debug, thiserror::Error)]
pub enum MockError {
    #[error("Operation error: {0}")]
    OpError(String),
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Sync + Send + 'static>),

}
#[derive(Debug, Default)]
pub struct MockFilesystem {
    copy_error:        Option<MockError>,//Option<Box<dyn Error + Send + Sync>>,
    create_dir_error:  Option<MockError>,//Option<Box<dyn Error + Send + Sync>>,
    create_file_error: Option<MockError>,//Option<Box<dyn Error + Send + Sync>>,
    open_file_error:   Option<MockError>,//Option<Box<dyn Error + Send + Sync>>,
    read_file_error:   Option<MockError>,//Option<Box<dyn Error + Send + Sync>>,
    write_error:       Option<MockError>,//Option<Box<dyn Error + Send + Sync>>,
    pub operations:    Vec<Operation>,
}

impl MockFilesystem {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_create_dir_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            copy_error:        None,
            create_dir_error:  Some(MockError::OpError("create_dir".into())),
            open_file_error:   None,
            create_file_error: None,
            read_file_error:   None,
            write_error:       None,
            operations:        vec![],
        }
    }

    fn with_create_file_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            copy_error:        None,
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: Some(MockError::OpError("create_file".into())),
            read_file_error:   None,
            write_error:       None,
            operations:        vec![],
        }
    }

    fn with_read_file_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            copy_error:        None,
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: None,
            read_file_error:   Some(MockError::OpError("read".into())),
            write_error:       None,
            operations:        vec![],
        }
    }

    fn with_copy_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            copy_error:        Some(MockError::OpError("copy".into())),
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: None,
            read_file_error:   None,
            write_error:       None,
            operations:        vec![],
        }
    }

    /// check crate: thisError for easier implementation of errors!
    fn with_write_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            copy_error:        None,
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: None,
            read_file_error:   None,
            write_error:       Some(MockError::OpError("write".into())),
            operations:        vec![],
        }
    }
}

#[async_trait]
impl FileSystem for MockFilesystem {
    type File = LocalFile;
    type FSError = MockError;

    async fn create_dir<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<(), Self::FSError> {
        if let Some(err) = self.create_dir_error.take() {
            return Err(err);
        }

        self.operations
            .push(Operation::CreateDir { path: path.as_ref().to_path_buf() });
        Ok(())
    }

    async fn write<P: AsRef<Path> + Send>(&mut self, path: P, content: impl Into<String> + Send) -> Result<(), Self::FSError> {
        if let Some(err) = self.write_error.take() {
            return Err(err);
        }

        self.operations.push(Operation::Write {
            path:    path.as_ref().to_path_buf(),
            content: content.into(),
        });
        Ok(())
    }

    async fn create<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<Self::File, Self::FSError> {
        if let Some(err) = self.create_file_error.take() {
            return Err(err);
        }

        let p = path.as_ref().to_path_buf();

        self.operations
            .push(Operation::CreateFile { path: p.clone() });

        let file = File::create(p).expect("not created");
        Ok(LocalFile::from(file))
    }

    async fn open_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<(),Self::FSError> {
        if let Some(err) = self.open_file_error.take() {
            return Err(err);
        }

        self.operations
            .push(Operation::OpenFile { path: path.as_ref().to_path_buf() });
        Ok(())
    }

    async fn read_file<P: AsRef<Path> + Send>(&mut self, path: P) -> Result<String, Self::FSError> {
        if let Some(err) = self.read_file_error.take() {
            return Err(err);
        }

        self.operations
            .push(Operation::ReadFile { path: path.as_ref().to_path_buf() });
        Ok("This is a test".to_owned())
    }

    async fn copy<P: AsRef<Path> + Send>(&mut self, from: P, to: P) -> std::result::Result<(), Self::FSError> {
        if let Some(err) = self.copy_error.take() {
            return Err(err);
        }

        self.operations.push(Operation::Copy {
            from: from.as_ref().to_path_buf(),
            to:   to.as_ref().to_path_buf(),
        });
        Ok(())
    }
}
