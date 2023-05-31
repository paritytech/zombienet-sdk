use std::{self, error::Error, fmt::Debug, fs};

use async_trait::async_trait;
use serde::Serialize;

use crate::shared::{
    constants::{DEFAULT_DATA_DIR, DEFAULT_REMOTE_DIR, LOCALHOST},
    provider::Provider,
    types::{NamespaceDef, NamespaceMetadata},
};

trait FileSystem {
    fn create_dir(&mut self, path: impl Into<String>) -> Result<(), Box<dyn Error>>;
    fn write(
        &mut self,
        path: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Serialize)]
struct FilesystemInMemory {}

impl FileSystem for FilesystemInMemory {
    fn create_dir(&mut self, path: impl Into<String>) -> Result<(), Box<dyn Error>> {
        fs::create_dir(path.into())?;
        Ok(())
    }

    fn write(
        &mut self,
        path: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<(), Box<dyn Error>> {
        fs::write(path.into(), content.into()).expect("Error writing file");
        Ok(())
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
struct NativeProvider<T: FileSystem + Debug> {
    /// Namespace of the client
    namespace:             String,
    /// path where configuration relies
    config_path:           String,
    // variable that shows if debug is activated
    debug:                 bool,
    /// the timeout for the client to exit
    timeout:               u32,
    /// command sent to client
    command:               String,
    /// temporary directory
    tmp_dir:               String,
    pod_monitor_available: bool,
    local_magic_file_path: String,
    remote_dir:            String,
    data_dir:              String,
    filesystem:            T,
}

impl<T: FileSystem + Debug> NativeProvider<T> {
    pub fn new(
        namespace: impl Into<String>,
        config_path: impl Into<String>,
        tmp_dir: impl Into<String>,
        filesystem: T,
    ) -> Self {
        let tmp_dir = tmp_dir.into();

        Self {
            namespace: namespace.into(),
            config_path: config_path.into(),
            debug: true,
            timeout: 60, // seconds
            local_magic_file_path: format!("{}/finished.txt", &tmp_dir),
            remote_dir: format!("{}{}", &tmp_dir, DEFAULT_REMOTE_DIR),
            data_dir: format!("{}{}", &tmp_dir, DEFAULT_DATA_DIR),
            command: "bash".into(),
            tmp_dir,
            pod_monitor_available: false,
            filesystem,
        }
    }
}

#[async_trait]
impl<T: FileSystem + Debug> Provider for NativeProvider<T> {
    fn create_namespace(&mut self) -> Result<(), Box<dyn Error>> {
        let name_space_def = NamespaceDef {
            api_version: "v1".into(),
            kind:        "Namespace".into(),
            metadata:    NamespaceMetadata {
                name:   format!("{}", &self.namespace),
                labels: None,
            },
        };

        let file_path = format!("{}/{}", &self.tmp_dir, "namespace");
        let content = serde_json::to_string(&name_space_def)?;

        self.filesystem.write(file_path, content)?;
        Ok(())
    }

    fn setup_cleaner(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn get_node_ip(&self) -> Result<String, Box<dyn Error>> {
        Ok(LOCALHOST.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_native_provider() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", FilesystemInMemory {});

        assert_eq!(native_provider.namespace, "something");
        assert_eq!(native_provider.config_path, "./");
        assert!(native_provider.debug);
        assert_eq!(native_provider.timeout, 60);
        assert_eq!(native_provider.tmp_dir, "./tmp");
        assert_eq!(native_provider.command, "bash");
        assert!(!native_provider.pod_monitor_available);
        assert_eq!(native_provider.local_magic_file_path, "./tmp/finished.txt");
        assert_eq!(native_provider.remote_dir, "./tmp/cfg");
        assert_eq!(native_provider.data_dir, "./tmp/data");
    }
}
