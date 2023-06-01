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
    types::{
        NamespaceDef, NamespaceMetadata, NativeRunCommandOptions, PodDef, RunCommandResponse,
        ZombieRole,
    },
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
    // Namespace of the client
    namespace:                String,
    // Path where configuration relies
    config_path:              String,
    // Variable that shows if debug is activated
    is_debug:                 bool,
    // The timeout for the client to exit
    timeout:                  u32,
    // Command sent to client
    command:                  String,
    // Temporary directory
    tmp_dir:                  String,
    is_pod_monitor_available: bool,
    local_magic_file_path:    String,
    remote_dir:               String,
    data_dir:                 String,
    filesystem:               T,
}

impl<T: FileSystem + Debug> NativeProvider<T> {
    pub fn new(
        namespace: impl Into<String>,
        config_path: impl Into<String>,
        tmp_dir: impl Into<String>,
        filesystem: T,
    ) -> Self {
        let tmp_dir: String = tmp_dir.into();

        Self {
            namespace: namespace.into(),
            config_path: config_path.into(),
            is_debug: true,
            timeout: 60, // seconds
            local_magic_file_path: format!("{}/finished.txt", &tmp_dir),
            remote_dir: format!("{}{}", &tmp_dir, DEFAULT_REMOTE_DIR),
            data_dir: format!("{}{}", &tmp_dir, DEFAULT_DATA_DIR),
            command: "bash".into(),
            tmp_dir,
            is_pod_monitor_available: false,
            filesystem,
        }
    }
}

#[async_trait]
impl<T: FileSystem + Debug> Provider for NativeProvider<T> {
    fn create_namespace(&mut self) -> Result<(), Box<dyn Error>> {
        let namespace_def: NamespaceDef = NamespaceDef {
            api_version: "v1".into(),
            kind:        "Namespace".into(),
            metadata:    NamespaceMetadata {
                name:   format!("{}", &self.namespace),
                labels: None,
            },
        };

        let file_path: String = format!("{}/namespace", &self.tmp_dir);
        let content: String = serde_json::to_string(&namespace_def)?;

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
        opts: NativeRunCommandOptions,
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

        if !output.stdout.is_empty() {
            return Ok(RunCommandResponse {
                exit_code: output.status,
                std_out:   output.stdout,
                std_err:   None,
            });
        } else if !output.stderr.is_empty() {
            if !opts.allow_fail {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Allow fail",
                )));
            };

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

    fn create_resource(
        &mut self,
        resourse_def: PodDef,
        scoped: bool,
        wait_ready: bool,
    ) -> Result<(), Box<dyn Error>> {
        let name: String = resourse_def.metadata.name.clone();
        let local_file_path: String = format!("{}/{}.yaml", &self.tmp_dir, name);

        let content: String = serde_json::to_string(&resourse_def)?;

        self.filesystem
            .write(local_file_path, content)
            .expect("Create source: Failed to write file");

        // match resourse_def.metadata.labels.zombie_role {
        //     ZombieRole::Temp => {
        //         self.run_command(
        //             resourse_def.spec.command.clone(),
        //             NativeRunCommandOptions {
        //                 allow_fail: Some(true),
        //             },
        //         )?;
        //     },
        //     _ => {
        //         self.run_command(
        //             vec!["bash".into(), local_file_path],
        //             NativeRunCommandOptions {
        //                 allow_fail: Some(true),
        //             },
        //         )?;
        //     },
        // }

        todo!()

        // if (resourseDef.metadata.labels["zombie-role"] === ZombieRole.Temp) {
        //   await this.runCommand(resourseDef.spec.command);
        // } else {
        //   if (resourseDef.spec.command[0] === "bash")
        //     resourseDef.spec.command.splice(0, 1);
        //   debug(this.command);
        //   debug(resourseDef.spec.command);

        //   const log = fs.createWriteStream(this.processMap[name].logs);
        //   const nodeProcess = spawn(
        //     this.command,
        //     ["-c", ...resourseDef.spec.command],
        //     { env: { ...process.env, ...resourseDef.spec.env } },
        //   );
        //   debug(nodeProcess.pid);
        //   nodeProcess.stdout.pipe(log);
        //   nodeProcess.stderr.pipe(log);
        //   this.processMap[name].pid = nodeProcess.pid;
        //   this.processMap[name].cmd = resourseDef.spec.command;

        //   await this.wait_node_ready(name);
        // }
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use super::*;
    use crate::helpers::{MockFilesystem, Operation};

    #[test]
    fn new_native_provider() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        assert_eq!(native_provider.namespace, "something");
        assert_eq!(native_provider.config_path, "./");
        assert!(native_provider.is_debug);
        assert_eq!(native_provider.timeout, 60);
        assert_eq!(native_provider.tmp_dir, "./tmp");
        assert_eq!(native_provider.command, "bash");
        assert!(!native_provider.is_pod_monitor_available);
        assert_eq!(native_provider.local_magic_file_path, "./tmp/finished.txt");
        assert_eq!(native_provider.remote_dir, "./tmp/cfg");
        assert_eq!(native_provider.data_dir, "./tmp/data");
    }

    #[test]
    fn test_fielsystem_usage() {
        let mut native_provider: NativeProvider<MockFilesystem> =
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
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        assert_eq!(native_provider.get_node_ip().unwrap(), LOCALHOST);
    }

    #[test]
    fn test_run_command_when_bash_is_removed() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let result: RunCommandResponse = native_provider
            .run_command(
                vec!["bash".into(), "ls".into()],
                NativeRunCommandOptions::default(),
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

        let result = native_provider.run_command(
            vec!["-c".into(), "ls".into()],
            NativeRunCommandOptions::default(),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_run_command_when_error_return_error() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let mut some = native_provider.run_command(
            vec!["ls".into(), "ls".into()],
            NativeRunCommandOptions::default(),
        );

        assert!(some.is_err());

        some = native_provider.run_command(
            vec!["ls".into(), "ls".into()],
            NativeRunCommandOptions { allow_fail: true },
        );

        assert!(some.is_ok());
    }
}
