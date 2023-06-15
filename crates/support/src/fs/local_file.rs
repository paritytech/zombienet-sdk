use std::{
    fs::File,
    io::{Read, Write},
    process::Stdio,
};

#[derive(Debug)]
pub struct LocalFile(File);

impl From<File> for LocalFile {
    fn from(file: File) -> Self {
        LocalFile(file)
    }
}

impl From<LocalFile> for Stdio {
    fn from(value: LocalFile) -> Self {
        value.0.into()
    }
}

impl Write for LocalFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.0.flush()
    }
}

impl Read for LocalFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}
