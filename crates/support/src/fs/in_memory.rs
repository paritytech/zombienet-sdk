use std::{collections::HashMap, ffi::OsString, path::Path, sync::Arc};

use super::{FileSystem, FileSystemResult};
use anyhow::anyhow;
use async_trait::async_trait;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub enum InMemoryFile {
    File(Vec<u8>),
    Directory,
}

#[derive(Default, Debug, Clone)]
pub struct InMemoryFileSystem {
    files: Arc<RwLock<HashMap<OsString, InMemoryFile>>>,
}

impl InMemoryFileSystem {
    pub fn new(files: HashMap<OsString, InMemoryFile>) -> Self {
        Self {
            files: Arc::new(RwLock::new(files)),
        }
    }
}

#[async_trait]
impl FileSystem for InMemoryFileSystem {
    async fn create_dir(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()> {
        let path = path.as_ref();
        let os_path = path.as_os_str();

        match self.files.read().await.get(os_path) {
            Some(InMemoryFile::File(_)) => {
                Err(anyhow!("file {:?} already exists", os_path.to_owned(),))?
            },
            Some(InMemoryFile::Directory) => {
                Err(anyhow!("directory {:?} already exists", os_path.to_owned(),))?
            },
            None => {},
        };

        let mut ancestors = path.ancestors().skip(1);
        while let Some(path) = ancestors.next() {
            match self.files.read().await.get(path.as_os_str()) {
                Some(InMemoryFile::Directory) => continue,
                Some(InMemoryFile::File(_)) => Err(anyhow!(
                    "ancestor {:?} is not a directory",
                    path.as_os_str(),
                ))?,
                None => Err(anyhow!("ancestor {:?} doesn't exists", path.as_os_str(),))?,
            };
        }

        self.files
            .write()
            .await
            .insert(os_path.to_owned(), InMemoryFile::Directory);

        Ok(())
    }

    async fn create_dir_all(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<()> {
        let path = path.as_ref();
        let mut files = self.files.write().await;
        let mut ancestors = path
            .ancestors()
            .collect::<Vec<&Path>>()
            .into_iter()
            .rev()
            .skip(1);

        while let Some(path) = ancestors.next() {
            match files.get(path.as_os_str()) {
                Some(InMemoryFile::Directory) => continue,
                Some(InMemoryFile::File(_)) => Err(anyhow!(
                    "ancestor {:?} is not a directory",
                    path.as_os_str().to_owned(),
                ))?,
                None => files.insert(path.as_os_str().to_owned(), InMemoryFile::Directory),
            };
        }

        Ok(())
    }

    async fn read(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<Vec<u8>> {
        let os_path = path.as_ref().as_os_str();

        match self.files.read().await.get(os_path) {
            Some(InMemoryFile::File(content)) => Ok(content.clone()),
            Some(InMemoryFile::Directory) => {
                Err(anyhow!("file {:?} is a directory", os_path).into())
            },
            None => Err(anyhow!("file {:?} not found", os_path).into()),
        }
    }

    async fn read_to_string(&self, path: impl AsRef<Path> + Send) -> FileSystemResult<String> {
        let os_path = path.as_ref().as_os_str().to_owned();
        let content = self.read(path).await?;

        String::from_utf8(content)
            .map_err(|_| anyhow!("invalid utf-8 encoding for file {:?}", os_path).into())
    }

    async fn write(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()> {
        let path = path.as_ref();
        let os_path = path.as_os_str();
        let mut files = self.files.write().await;

        let mut ancestors = path.ancestors().skip(1);
        while let Some(path) = ancestors.next() {
            match files.get(path.as_os_str()) {
                Some(InMemoryFile::Directory) => continue,
                Some(InMemoryFile::File(_)) => Err(anyhow!(
                    "ancestor {:?} is not a directory",
                    path.as_os_str()
                ))?,
                None => Err(anyhow!("ancestor {:?} doesn't exists", path.as_os_str()))?,
            };
        }

        if let Some(InMemoryFile::Directory) = files.get(os_path) {
            return Err(anyhow!("file {:?} is a directory", os_path).into());
        }

        files.insert(
            os_path.to_owned(),
            InMemoryFile::File(contents.as_ref().to_vec()),
        );

        Ok(())
    }

    async fn append(
        &self,
        path: impl AsRef<Path> + Send,
        contents: impl AsRef<[u8]> + Send,
    ) -> FileSystemResult<()> {
        let path = path.as_ref();
        let mut existing_contents = match self.read(path).await {
            Ok(existing_contents) => existing_contents,
            Err(err) if err.to_string() == format!("file {:?} not found", path.as_os_str()) => {
                vec![]
            },
            Err(err) => Err(err)?,
        };
        existing_contents.append(&mut contents.as_ref().to_vec());

        self.write(path, existing_contents).await
    }

    async fn copy(
        &self,
        from: impl AsRef<Path> + Send,
        to: impl AsRef<Path> + Send,
    ) -> FileSystemResult<()> {
        let content = self.read(from).await?;
        self.write(to, content).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn create_dir_should_create_a_directory_at_root() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/").unwrap(),
            InMemoryFile::Directory,
        )]));

        fs.create_dir("/dir").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 2);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/dir").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
    }

    #[tokio::test]
    async fn create_dir_should_return_an_error_if_directory_already_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (OsString::from_str("/dir").unwrap(), InMemoryFile::Directory),
        ]));

        let err = fs.create_dir("/dir").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 2);
        assert_eq!(err.to_string(), "directory \"/dir\" already exists");
    }

    #[tokio::test]
    async fn create_dir_should_return_an_error_if_file_already_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/dir").unwrap(),
                InMemoryFile::File(vec![]),
            ),
        ]));

        let err = fs.create_dir("/dir").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 2);
        assert_eq!(err.to_string(), "file \"/dir\" already exists");
    }

    #[tokio::test]
    async fn create_dir_should_create_a_directory_if_all_ancestors_exist() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::Directory,
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
            (
                OsString::from_str("/path/to/my").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        fs.create_dir("/path/to/my/dir").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 5);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path/to/my/dir").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
    }

    #[tokio::test]
    async fn create_dir_should_return_an_error_if_some_directory_ancestor_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::Directory,
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs.create_dir("/path/to/my/dir").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 3);
        assert_eq!(err.to_string(), "ancestor \"/path/to/my\" doesn't exists");
    }

    #[tokio::test]
    async fn create_dir_should_return_an_error_if_some_ancestor_is_not_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::File(vec![]),
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
            (
                OsString::from_str("/path/to/my").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs.create_dir("/path/to/my/dir").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 4);
        assert_eq!(err.to_string(), "ancestor \"/path\" is not a directory");
    }

    #[tokio::test]
    async fn create_dir_all_should_create_a_directory_and_all_its_ancestors_if_they_dont_exist() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/").unwrap(),
            InMemoryFile::Directory,
        )]));

        fs.create_dir_all("/path/to/my/dir").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 5);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path/to").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path/to/my").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path/to/my/dir").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
    }

    #[tokio::test]
    async fn create_dir_all_should_create_a_directory_and_some_of_its_ancestors_if_they_dont_exist()
    {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::Directory,
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        fs.create_dir_all("/path/to/my/dir").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 5);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path/to/my").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/path/to/my/dir").unwrap())
                .unwrap(),
            InMemoryFile::Directory
        ));
    }

    #[tokio::test]
    async fn create_dir_all_should_return_an_error_if_some_ancestor_is_not_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::File(vec![]),
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs.create_dir_all("/path/to/my/dir").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 3);
        assert_eq!(err.to_string(), "ancestor \"/path\" is not a directory");
    }

    #[tokio::test]
    async fn read_should_return_the_file_content() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/myfile").unwrap(),
            InMemoryFile::File("content".as_bytes().to_vec()),
        )]));

        let content = fs.read("/myfile").await.unwrap();

        assert_eq!(content, "content".as_bytes().to_vec());
    }

    #[tokio::test]
    async fn read_should_return_an_error_if_file_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::new());

        let err = fs.read("/myfile").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" not found");
    }

    #[tokio::test]
    async fn read_should_return_an_error_if_file_is_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/myfile").unwrap(),
            InMemoryFile::Directory,
        )]));

        let err = fs.read("/myfile").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" is a directory");
    }

    #[tokio::test]
    async fn read_to_string_should_return_the_file_content_as_a_string() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/myfile").unwrap(),
            InMemoryFile::File("content".as_bytes().to_vec()),
        )]));

        let content = fs.read_to_string("/myfile").await.unwrap();

        assert_eq!(content, "content");
    }

    #[tokio::test]
    async fn read_to_string_should_return_an_error_if_file_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::new());

        let err = fs.read_to_string("/myfile").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" not found");
    }

    #[tokio::test]
    async fn read_to_string_should_return_an_error_if_file_is_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/myfile").unwrap(),
            InMemoryFile::Directory,
        )]));

        let err = fs.read_to_string("/myfile").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" is a directory");
    }

    #[tokio::test]
    async fn read_to_string_should_return_an_error_if_file_isnt_utf8_encoded() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/myfile").unwrap(),
            InMemoryFile::File(vec![0xC3, 0x28]),
        )]));

        let err = fs.read_to_string("/myfile").await.unwrap_err();

        assert_eq!(
            err.to_string(),
            "invalid utf-8 encoding for file \"/myfile\""
        );
    }

    #[tokio::test]
    async fn write_should_create_file_with_content_if_file_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/").unwrap(),
            InMemoryFile::Directory,
        )]));

        fs.write("/myfile", "my file content").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 2);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/myfile").unwrap()),
            Some(InMemoryFile::File(content)) if content == "my file content".as_bytes()
        ));
    }

    #[tokio::test]
    async fn write_should_overwrite_file_content_if_file_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
        ]));

        fs.write("/myfile", "my new file content").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 2);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/myfile").unwrap()),
            Some(InMemoryFile::File(content)) if content == "my new file content".as_bytes()
        ));
    }

    #[tokio::test]
    async fn write_should_return_an_error_if_file_is_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs.write("/myfile", "my file content").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 2);
        assert_eq!(err.to_string(), "file \"/myfile\" is a directory");
    }

    #[tokio::test]
    async fn write_should_return_an_error_if_file_is_new_and_some_ancestor_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs
            .write("/path/to/myfile", "my file content")
            .await
            .unwrap_err();

        assert_eq!(fs.files.read().await.len(), 2);
        assert_eq!(err.to_string(), "ancestor \"/path\" doesn't exists");
    }

    #[tokio::test]
    async fn write_should_return_an_error_if_file_is_new_and_some_ancestor_is_not_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::File(vec![]),
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs
            .write("/path/to/myfile", "my file content")
            .await
            .unwrap_err();

        assert_eq!(fs.files.read().await.len(), 3);
        assert_eq!(err.to_string(), "ancestor \"/path\" is not a directory");
    }

    #[tokio::test]
    async fn append_should_update_file_content_if_file_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
        ]));

        fs.append("/myfile", " has been updated with new things")
            .await
            .unwrap();

        assert_eq!(fs.files.read().await.len(), 2);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/myfile").unwrap()),
            Some(InMemoryFile::File(content)) if content == "my file content has been updated with new things".as_bytes()
        ));
    }

    #[tokio::test]
    async fn append_should_create_file_with_content_if_file_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/").unwrap(),
            InMemoryFile::Directory,
        )]));

        fs.append("/myfile", "my file content").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 2);
        assert!(matches!(
            fs.files
                .read()
                .await
                .get(&OsString::from_str("/myfile").unwrap()),
            Some(InMemoryFile::File(content)) if content == "my file content".as_bytes()
        ));
    }

    #[tokio::test]
    async fn append_should_return_an_error_if_file_is_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/myfile").unwrap(),
            InMemoryFile::Directory,
        )]));

        let err = fs.append("/myfile", "my file content").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" is a directory");
    }

    #[tokio::test]
    async fn append_should_return_an_error_if_file_is_new_and_some_ancestor_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs
            .append("/path/to/myfile", "my file content")
            .await
            .unwrap_err();

        assert_eq!(fs.files.read().await.len(), 2);
        assert_eq!(err.to_string(), "ancestor \"/path\" doesn't exists");
    }

    #[tokio::test]
    async fn append_should_return_an_error_if_file_is_new_and_some_ancestor_is_not_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/path").unwrap(),
                InMemoryFile::File(vec![]),
            ),
            (
                OsString::from_str("/path/to").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs
            .append("/path/to/myfile", "my file content")
            .await
            .unwrap_err();

        assert_eq!(fs.files.read().await.len(), 3);
        assert_eq!(err.to_string(), "ancestor \"/path\" is not a directory");
    }

    #[tokio::test]
    async fn copy_should_creates_new_destination_file_if_it_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
        ]));

        fs.copy("/myfile", "/myfilecopy").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 3);
        assert!(
            matches!(fs.files.read().await.get(&OsString::from_str("/myfilecopy").unwrap()).unwrap(), InMemoryFile::File(content) if content == "my file content".as_bytes())
        );
    }

    #[tokio::test]
    async fn copy_should_updates_the_file_content_of_the_destination_file_if_it_already_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my new file content".as_bytes().to_vec()),
            ),
            (
                OsString::from_str("/myfilecopy").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
        ]));

        fs.copy("/myfile", "/myfilecopy").await.unwrap();

        assert_eq!(fs.files.read().await.len(), 3);
        assert!(
            matches!(fs.files.read().await.get(&OsString::from_str("/myfilecopy").unwrap()).unwrap(), InMemoryFile::File(content) if content == "my new file content".as_bytes())
        );
    }

    #[tokio::test]
    async fn copy_should_returns_an_error_if_source_file_doesnt_exists() {
        let fs = InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/").unwrap(),
            InMemoryFile::Directory,
        )]));

        let err = fs.copy("/myfile", "/mfilecopy").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" not found");
    }

    #[tokio::test]
    async fn copy_should_returns_an_error_if_source_file_is_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs.copy("/myfile", "/mfilecopy").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfile\" is a directory");
    }

    #[tokio::test]
    async fn copy_should_returns_an_error_if_destination_file_is_a_directory() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
            (
                OsString::from_str("/myfilecopy").unwrap(),
                InMemoryFile::Directory,
            ),
        ]));

        let err = fs.copy("/myfile", "/myfilecopy").await.unwrap_err();

        assert_eq!(err.to_string(), "file \"/myfilecopy\" is a directory");
    }

    #[tokio::test]
    async fn copy_should_returns_an_error_if_destination_file_is_new_and_some_ancestor_doesnt_exists(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
        ]));

        let err = fs.copy("/myfile", "/somedir/myfilecopy").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 2);
        assert_eq!(err.to_string(), "ancestor \"/somedir\" doesn't exists");
    }

    #[tokio::test]
    async fn copy_should_returns_an_error_if_destination_file_is_new_and_some_ancestor_is_not_a_directory(
    ) {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::Directory),
            (
                OsString::from_str("/myfile").unwrap(),
                InMemoryFile::File("my file content".as_bytes().to_vec()),
            ),
            (
                OsString::from_str("/mypath").unwrap(),
                InMemoryFile::File(vec![]),
            ),
        ]));

        let err = fs.copy("/myfile", "/mypath/myfilecopy").await.unwrap_err();

        assert_eq!(fs.files.read().await.len(), 3);
        assert_eq!(err.to_string(), "ancestor \"/mypath\" is not a directory");
    }
}