use std::{os::unix::fs::PermissionsExt, path::Path};
use tokio::io::AsyncWriteExt;

use async_trait::async_trait;
use uuid::Uuid;

use super::{FileSystem, FileSystemError, FileSystemResult};

#[derive(Default, Debug, Clone)]
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
            .create(true)
            .append(true)
            .open(path)
            .await
            .map_err(Into::<FileSystemError>::into)?;

        file.write_all(contents)
            .await
            .map_err(Into::<FileSystemError>::into)?;

        file.flush().await.and(Ok(())).map_err(Into::into)
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

    async fn set_mode(&self, path: impl AsRef<Path> + Send, mode: u32) -> FileSystemResult<()> {
        // because we can't create a Permissions struct directly, we create a temporary empty file and retrieve the
        // Permissions from it, we then modify its mode and apply it to our file
        let temp_file_path = format!(
            "{}/{}",
            std::env::temp_dir().to_string_lossy(),
            Uuid::new_v4()
        );
        let temp_file =
            std::fs::File::create(temp_file_path).map_err(Into::<FileSystemError>::into)?;

        let mut permissions = temp_file
            .metadata()
            .map_err(Into::<FileSystemError>::into)?
            .permissions();
        permissions.set_mode(mode);

        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const FILE_BITS: u32 = 0o100000;
    const DIR_BITS: u32 = 0o40000;

    fn setup() -> String {
        let test_dir = format!("/tmp/unit_test_{}", Uuid::new_v4());
        std::fs::create_dir(&test_dir).unwrap();
        test_dir
    }

    fn teardown(test_dir: String) {
        std::fs::remove_dir_all(&test_dir).unwrap();
    }

    #[tokio::test]
    async fn create_dir_should_create_a_new_directory_at_path() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let new_dir = format!("{test_dir}/mynewdir");
        fs.create_dir(&new_dir).await.unwrap();

        let new_dir_path = Path::new(&new_dir);
        assert!(new_dir_path.exists() && new_dir_path.is_dir());
        teardown(test_dir);
    }

    #[tokio::test]
    async fn create_dir_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let new_dir = format!("{test_dir}/mynewdir");
        // intentionally create new dir before calling function to force error
        std::fs::create_dir(&new_dir).unwrap();
        let err = fs.create_dir(&new_dir).await.unwrap_err();

        assert_eq!(err.to_string(), "File exists (os error 17)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn create_dir_all_should_create_a_new_directory_and_all_of_it_ancestors_at_path() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let new_dir = format!("{test_dir}/the/path/to/mynewdir");
        fs.create_dir_all(&new_dir).await.unwrap();

        let new_dir_path = Path::new(&new_dir);
        assert!(new_dir_path.exists() && new_dir_path.is_dir());
        teardown(test_dir);
    }

    #[tokio::test]
    async fn create_dir_all_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let new_dir = format!("{test_dir}/the/path/to/mynewdir");
        // intentionally create new file as ancestor before calling function to force error
        std::fs::write(format!("{test_dir}/the"), b"test").unwrap();
        let err = fs.create_dir_all(&new_dir).await.unwrap_err();

        assert_eq!(err.to_string(), "Not a directory (os error 20)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn read_should_return_the_contents_of_the_file_at_path() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        std::fs::write(&file_path, b"Test").unwrap();
        let contents = fs.read(file_path).await.unwrap();

        assert_eq!(contents, b"Test");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn read_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        // intentionally forget to create file to force error
        let err = fs.read(file_path).await.unwrap_err();

        assert_eq!(err.to_string(), "No such file or directory (os error 2)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn read_to_string_should_return_the_contents_of_the_file_at_path_as_string() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        std::fs::write(&file_path, b"Test").unwrap();
        let contents = fs.read_to_string(file_path).await.unwrap();

        assert_eq!(contents, "Test");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn read_to_string_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        // intentionally forget to create file to force error
        let err = fs.read_to_string(file_path).await.unwrap_err();

        assert_eq!(err.to_string(), "No such file or directory (os error 2)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn write_should_create_a_new_file_at_path_with_contents() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        fs.write(&file_path, "Test").await.unwrap();

        assert_eq!(std::fs::read_to_string(file_path).unwrap(), "Test");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn write_should_overwrite_an_existing_file_with_contents() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        std::fs::write(&file_path, "Test").unwrap();
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "Test");
        fs.write(&file_path, "Test updated").await.unwrap();

        assert_eq!(std::fs::read_to_string(file_path).unwrap(), "Test updated");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn write_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        // intentionally create directory instead of file to force error
        std::fs::create_dir(&file_path).unwrap();
        let err = fs.write(&file_path, "Test").await.unwrap_err();

        assert_eq!(err.to_string(), "Is a directory (os error 21)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn append_should_create_a_new_file_at_path_with_contents() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        fs.append(&file_path, "Test").await.unwrap();

        assert_eq!(std::fs::read_to_string(file_path).unwrap(), "Test");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn append_should_updates_an_existing_file_by_appending_contents() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        std::fs::write(&file_path, "Test").unwrap();
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "Test");
        fs.append(&file_path, " updated").await.unwrap();

        assert_eq!(std::fs::read_to_string(file_path).unwrap(), "Test updated");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn append_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let file_path = format!("{test_dir}/myfile");
        // intentionally create directory instead of file to force error
        std::fs::create_dir(&file_path).unwrap();
        let err = fs.append(&file_path, "Test").await.unwrap_err();

        assert_eq!(err.to_string(), "Is a directory (os error 21)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn copy_should_create_a_duplicate_of_source() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let from_path = format!("{test_dir}/myfile");
        std::fs::write(&from_path, "Test").unwrap();
        let to_path = format!("{test_dir}/mycopy");
        fs.copy(&from_path, &to_path).await.unwrap();

        assert_eq!(std::fs::read_to_string(to_path).unwrap(), "Test");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn copy_should_ovewrite_destination_if_alread_exists() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let from_path = format!("{test_dir}/myfile");
        std::fs::write(&from_path, "Test").unwrap();
        let to_path = format!("{test_dir}/mycopy");
        std::fs::write(&from_path, "Some content").unwrap();
        fs.copy(&from_path, &to_path).await.unwrap();

        assert_eq!(std::fs::read_to_string(to_path).unwrap(), "Some content");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn copy_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();

        let from_path = format!("{test_dir}/nonexistentfile");
        let to_path = format!("{test_dir}/mycopy");
        let err = fs.copy(&from_path, &to_path).await.unwrap_err();

        assert_eq!(err.to_string(), "No such file or directory (os error 2)");
        teardown(test_dir);
    }

    #[tokio::test]
    async fn set_mode_should_update_the_file_mode_at_path() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();
        let path = format!("{test_dir}/myfile");
        std::fs::write(&path, "Test").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode(),
            FILE_BITS + 0o664
        );

        fs.set_mode(&path, 0o400).await.unwrap();

        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode(),
            FILE_BITS + 0o400
        );
        teardown(test_dir);
    }

    #[tokio::test]
    async fn set_mode_should_update_the_directory_mode_at_path() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();
        let path = format!("{test_dir}/mydir");
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode(),
            DIR_BITS + 0o775
        );

        fs.set_mode(&path, 0o700).await.unwrap();

        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode(),
            DIR_BITS + 0o700
        );
        teardown(test_dir);
    }

    #[tokio::test]
    async fn set_mode_should_bubble_up_error_if_some_happens() {
        let test_dir = setup();
        let fs = LocalFileSystem::default();
        let path = format!("{test_dir}/somemissingfile");
        // intentionnally don't create file

        let err = fs.set_mode(&path, 0o400).await.unwrap_err();

        assert_eq!(err.to_string(), "No such file or directory (os error 2)");
        teardown(test_dir);
    }
}
