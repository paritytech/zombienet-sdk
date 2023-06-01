use std::{
    self,
    error::Error,
    fmt::Debug,
    process::{Command, Output},
};

use async_trait::async_trait;
use serde::Serialize;

use crate::shared::{
    constants::{DEFAULT_DATA_DIR, DEFAULT_REMOTE_DIR, LOCALHOST},
    provider::Provider,
    types::{NamespaceDef, NamespaceMetadata, RunCommandOptions, RunCommandResponse},
};

pub trait FileSystem {
    fn create_dir(&mut self, path: impl Into<String>) -> Result<(), Box<dyn Error>>;
    fn write(
        &mut self,
        path: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Serialize)]
struct FilesystemInMemory {}

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
        self.filesystem.create_dir(&self.remote_dir)?;
        Ok(())
    }

    fn setup_cleaner(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn get_node_ip(&self) -> Result<String, Box<dyn Error>> {
        Ok(LOCALHOST.to_owned())
    }

    fn run_command(
        &self,
        mut args: Vec<String>,
        opts: RunCommandOptions,
    ) -> Result<RunCommandResponse, Box<dyn Error>> {
        if let Some(arg) = args.get(0) {
            if arg == "bash" {
                args.remove(0);
            }
        }

        let output: Output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(args)
                .output()
                .expect("failed to execute process")
        } else {
            if let Some(arg) = args.get(0) {
                if arg == "-c" {
                    args.remove(0);
                }
            }

            Command::new("sh")
                .arg("-c")
                .arg(args.join(" "))
                .output()
                .expect("failed to execute process")
        };

        if opts.allow_fail.is_some() && opts.allow_fail.unwrap() {
            panic!("{}", String::from_utf8(output.stderr).unwrap());
        }

        if !output.stdout.is_empty() {
            return Ok(RunCommandResponse {
                exit_code: output.status,
                std_out:   output.stdout,
                std_err:   None,
            });
        } else if !output.stderr.is_empty() {
            return Ok(RunCommandResponse {
                exit_code: output.status,
                std_out:   output.stdout,
                std_err:   Some(output.stderr),
            });
        }

        Ok(RunCommandResponse {
            exit_code: output.status,
            std_out:   output.stdout,
            std_err:   Some(output.stderr),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use super::*;
    use crate::helpers::{MockFilesystem, Operation};

    #[test]
    fn new_native_provider() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

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

    #[test]
    fn test_fielsystem_usage() {
        let mut native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        native_provider.create_namespace().unwrap();

        assert!(native_provider.filesystem.operations.len() == 2);

        assert_eq!(
            native_provider.filesystem.operations[0],
            Operation::CreateFile {
                path:    "./tmp/namespace".into(),
                content: r#"{"api_version":"v1","kind":"Namespace","metadata":{"name":"something","labels":null}}"#.into(),
            }
        );

        assert_eq!(
            native_provider.filesystem.operations[1],
            Operation::CreateDir {
                path: "./tmp/cfg".into(),
            }
        );
    }

    #[test]
    fn test_get_node_ip() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        assert_eq!(native_provider.get_node_ip().unwrap(), LOCALHOST);
    }

    #[test]
    fn test_run_command_when_bash_is_removed() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let result = native_provider
            .run_command(
                vec!["bash".into(), "ls".into()],
                RunCommandOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result,
            RunCommandResponse {
                exit_code: ExitStatus::from_raw(0),
                std_out:   "Cargo.toml\nsrc\n".into(),
                std_err:   None,
            }
        );
    }

    #[test]
    fn test_run_command_when_dash_c_is_provided() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let result = native_provider
            .run_command(vec!["-c".into(), "ls".into()], RunCommandOptions::default())
            .unwrap();

        assert_eq!(
            result,
            RunCommandResponse {
                exit_code: ExitStatus::from_raw(0),
                std_out:   "Cargo.toml\nsrc\n".into(),
                std_err:   None,
            }
        );
    }

    #[test]
    #[should_panic]
    fn test_run_command_when_error_panic() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        native_provider
            .run_command(
                vec!["echo".into(), "ls".into()],
                RunCommandOptions {
                    resource_def: None,
                    scoped:       None,
                    allow_fail:   Some(true),
                    main_cmd:     String::new(),
                },
            )
            .unwrap();
    }
}
