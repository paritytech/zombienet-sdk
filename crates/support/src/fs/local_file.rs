use std::{ fs::File, io::Write };

#[derive(Debug)]
pub struct LocalFile(File);

impl From<File> for LocalFile {
    fn from(file: File) -> Self {
        LocalFile(file)
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