use std::error::Error;

use crate::native::FileSystem;

#[derive(Debug, PartialEq)]
pub enum Operation {
    // ReadFile,
    // DeleteFile { path: String },
    // DeleteDir,
    // LinkFile,
    CreateFile { path: String, content: String },
    CreateDir { path: String },
}

#[derive(Debug)]
pub struct MockFilesystem {
    create_dir_error: Option<Box<dyn Error>>,
    write_error:      Option<Box<dyn Error>>,
    pub operations:   Vec<Operation>,
}

impl MockFilesystem {
    pub fn new() -> Self {
        Self {
            create_dir_error: None,
            write_error:      None,
            operations:       vec![],
        }
    }

    fn with_create_dir_error(error: impl Error + 'static) -> Self {
        Self {
            create_dir_error: Some(Box::new(error)),
            write_error:      None,
            operations:       vec![],
        }
    }

    fn with_write_error(error: impl Error + 'static) -> Self {
        Self {
            create_dir_error: None,
            write_error:      Some(Box::new(error)),
            operations:       vec![],
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

        self.operations.push(Operation::CreateFile {
            path:    path.into(),
            content: content.into(),
        });
        Ok(())
    }
}
