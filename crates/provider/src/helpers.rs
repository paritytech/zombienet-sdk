use std::{error::Error, fs::File};

use crate::{native::FileSystem, shared::types::LocalFile};

#[derive(Debug, PartialEq)]
pub enum Operation {
    // ReadFile,
    // DeleteFile { path: String },
    // DeleteDir,
    // LinkFile,
    CreateFile { path: String },
    CreateDir { path: String },
    OpenFile { path: String },
    Write { path: String, content: String },
}

#[derive(Debug)]
pub struct MockFilesystem {
    create_dir_error:  Option<Box<dyn Error + Send + Sync>>,
    create_file_error: Option<Box<dyn Error + Send + Sync>>,
    open_file_error:   Option<Box<dyn Error + Send + Sync>>,
    write_error:       Option<Box<dyn Error + Send + Sync>>,
    pub operations:    Vec<Operation>,
}

impl MockFilesystem {
    pub fn new() -> Self {
        Self {
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: None,
            write_error:       None,
            operations:        vec![],
        }
    }

    fn with_create_dir_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            create_dir_error:  Some(Box::new(error)),
            open_file_error:   None,
            create_file_error: None,
            write_error:       None,
            operations:        vec![],
        }
    }

    fn with_create_file_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: Some(Box::new(error)),
            write_error:       None,
            operations:        vec![],
        }
    }

    /// check crate: thisError for easier implementation of errors!
    fn with_write_error(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            create_dir_error:  None,
            open_file_error:   None,
            create_file_error: None,
            write_error:       Some(Box::new(error)),
            operations:        vec![],
        }
    }
}

impl FileSystem for MockFilesystem {
    fn create_dir(&mut self, path: impl Into<String>) -> Result<(), Box<dyn Error>> {
        if let Some(err) = self.create_dir_error.take() {
            return Err(err);
        }

        self.operations
            .push(Operation::CreateDir { path: path.into() });
        Ok(())
    }

    fn write(
        &mut self,
        path: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(err) = self.write_error.take() {
            return Err(err);
        }

        self.operations.push(Operation::Write {
            path:    path.into(),
            content: content.into(),
        });
        Ok(())
    }

    fn create(&mut self, path: impl Into<String>) -> Result<LocalFile, Box<dyn Error>> {
        if let Some(err) = self.create_file_error.take() {
            return Err(err);
        }

        let p = format!("{}", &path.into());

        self.operations
            .push(Operation::CreateFile { path: p.clone() });

        let file = File::create(p).expect("not created");
        Ok(LocalFile::from(file))
    }

    fn open_file(&mut self, path: impl Into<String>) -> Result<(), Box<dyn Error>> {
        if let Some(err) = self.open_file_error.take() {
            return Err(err);
        }

        self.operations
            .push(Operation::OpenFile { path: path.into() });
        Ok(())
    }
}
