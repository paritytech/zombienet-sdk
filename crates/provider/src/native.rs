use std::{
    self,
    collections::HashMap,
    env,
    fmt::Debug,
    net::IpAddr,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::Serialize;
use support::fs::FileSystem;
use tokio::process::Command;

use super::Provider;
use crate::{
    errors::ProviderError,
    shared::{
        constants::{DEFAULT_DATA_DIR, DEFAULT_REMOTE_DIR, LOCALHOST, P2P_PORT},
        types::{FileMap, NativeRunCommandOptions, Node, Port, Process, RunCommandResponse},
    },
};
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct NativeProvider<T: FileSystem + Send + Sync> {
    // Namespace of the client (isolation directory)
    namespace: String,
    // Path where configuration relies, all the `files` are accessed relative to this.
    config_path: String,
    // Variable that shows if debug is activated
    is_debug: bool,
    // The timeout for start the node
    timeout: u32,
    // Command to use, e.g "bash"
    command: String,
    // Temporary directory, root directory for the network
    tmp_dir: String,
    local_magic_file_path: String,
    remote_dir: String,
    data_dir: String,
    process_map: HashMap<String, Process>,
    filesystem: T,
}

impl<T: FileSystem + Send + Sync> NativeProvider<T> {
    /// Zombienet `native` provider allows to run the nodes as a local process in the local environment
    /// params:
    ///   namespace:  Namespace of the clien
    ///   config_path: Path where configuration relies
    ///   tmp_dir: Temporary directory where files will be placed
    ///   filesystem: Filesystem to use (std::fs::FileSystem, mock etc.)
    pub fn new(
        namespace: impl Into<String>,
        config_path: impl Into<String>,
        tmp_dir: impl Into<String>,
        filesystem: T,
    ) -> Self {
        let tmp_dir: String = tmp_dir.into();
        let process_map: HashMap<String, Process> = HashMap::new();

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
            process_map,
            filesystem,
        }
    }
}

#[async_trait]
impl<T: FileSystem + Send + Sync> Provider for NativeProvider<T> {
    async fn create_namespace(&mut self) -> Result<(), ProviderError> {
        // Native provider don't have the `namespace` isolation.
        // but we create the `remoteDir` to place files
        self.filesystem
            .create_dir(&self.remote_dir)
            .await
            .map_err(|e| ProviderError::FSError(Box::new(e)))?;
        Ok(())
    }

    async fn destroy_namespace(&self) -> Result<(), ProviderError> {
        // get pids to kill all related process
        let pids: Vec<String> = self
            .process_map
            .iter()
            .filter(|(_, process)| process.pid != 0)
            .map(|(_, process)| process.pid.to_string())
            .collect();

        // TODO: use a crate (or even std) to get this info instead of relying on bash
        let result = self
            .run_command(
                [format!(
                    "ps ax| awk '{{print $1}}'| grep -E '{}'",
                    pids.join("|")
                )]
                .to_vec(),
                NativeRunCommandOptions {
                    is_failure_allowed: true,
                },
            )
            .await
            .unwrap();

        if result.exit_code.code().unwrap() == 0 {
            let pids_to_kill: Vec<String> = result
                .std_out
                .split(|c| c == '\n')
                .map(|s| s.into())
                .collect();

            let _ = self
                .run_command(
                    [format!("kill -9 {}", pids_to_kill.join(" "))].to_vec(),
                    NativeRunCommandOptions {
                        is_failure_allowed: true,
                    },
                )
                .await?;
        }
        Ok(())
    }

    async fn spawn_node(
        &mut self,
        _node: Node,
        _files_inject: Vec<FileMap>,
        _keystore: &str,
        _db_snapshot: &str,
    ) -> Result<(), ProviderError> {
        // TODO: We should implement the logic to go from the `Node` (nodeSpec)
        // to the running node, since we will no expose anymore the underline `Def`.
        // We can follow the logic of the spawn_from_def later.

        Ok(())
    }

    async fn spawn_temp(
        &self,
        _node: Node,
        _files_inject: Vec<FileMap>,
        _files_get: Vec<FileMap>,
    ) -> Result<(), ProviderError> {
        // TODO: We should implement the logic to go from the `Node` (nodeSpec)
        // to the running node, since we will no expose anymore the underline `Def`.
        // We can follow the logic of the spawn_from_def later.

        Ok(())
    }

    async fn copy_file_from_node(
        &mut self,
        pod_file_path: PathBuf,
        local_file_path: PathBuf,
    ) -> Result<(), ProviderError> {
        // TODO: log::debug!(format!("cp {} {}", pod_file_path, local_file_path));

        self.filesystem
            .copy(&pod_file_path, &local_file_path)
            .await
            .map_err(|e| ProviderError::FSError(Box::new(e)))?;
        Ok(())
    }

    async fn run_command(
        &self,
        mut args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, ProviderError> {
        if let Some(arg) = args.get(0) {
            if arg == "bash" {
                args.remove(0);
            }
        }

        // -c is already used in the process::Command to execute the command thus
        // needs to be removed in case provided
        if let Some(arg) = args.get(0) {
            if arg == "-c" {
                args.remove(0);
            }
        }

        let result = Command::new(&self.command)
            .arg("-c")
            .arg(args.join(" "))
            .output()
            .await?;

        if !result.status.success() && !opts.is_failure_allowed {
            return Err(ProviderError::RunCommandError(args.join(" ")));
        } else {
            // cmd success or we allow to fail
            // in either case we return Ok
            Ok(RunCommandResponse {
                exit_code: result.status,
                std_out: String::from_utf8_lossy(&result.stdout).into(),
                std_err: if result.stderr.is_empty() {
                    None
                } else {
                    Some(String::from_utf8_lossy(&result.stderr).into())
                },
            })
        }
    }

    // TODO: Add test
    async fn run_script(
        &mut self,
        identifier: String,
        script_path: String,
        args: Vec<String>,
    ) -> Result<RunCommandResponse, ProviderError> {
        let script_filename = Path::new(&script_path)
            .file_name()
            .ok_or(ProviderError::InvalidScriptPath(script_path.clone()))?
            .to_str()
            .ok_or(ProviderError::InvalidScriptPath(script_path.clone()))?;
        let script_path_in_pod = format!("{}/{}/{}", self.tmp_dir, identifier, script_filename);

        // upload the script
        self.filesystem
            .copy(&script_path, &script_path_in_pod)
            .await
            .map_err(|e| ProviderError::FSError(Box::new(e)))?;

        // set as executable
        self.run_command(
            vec![
                "chmod".to_owned(),
                "+x".to_owned(),
                script_path_in_pod.clone(),
            ],
            NativeRunCommandOptions::default(),
        )
        .await?;

        let command = format!(
            "cd {}/{} && {} {}",
            self.tmp_dir,
            identifier,
            script_path_in_pod,
            args.join(" ")
        );
        let result = self
            .run_command(vec![command], NativeRunCommandOptions::default())
            .await?;

        Ok(RunCommandResponse {
            exit_code: result.exit_code,
            std_out: result.std_out,
            std_err: result.std_err,
        })
    }

    // TODO: Add test
    async fn get_node_logs(&mut self, name: &str) -> Result<String, ProviderError> {
        // For now in native let's just return all the logs
        let result = self
            .filesystem
            .read_file(&format!("{}/{}.log", self.tmp_dir, name))
            .await
            .map_err(|e| ProviderError::FSError(Box::new(e)))?;
        return Ok(result);
    }

    async fn dump_logs(&mut self, path: String, pod_name: String) -> Result<(), ProviderError> {
        let dst_file_name: String = format!("{}/logs/{}.log", path, pod_name);
        let _ = self
            .filesystem
            .copy(
                &format!("{}/{}.log", self.tmp_dir, pod_name),
                &dst_file_name,
            )
            .await;
        Ok(())
    }

    async fn get_logs_command(&self, name: &str) -> Result<String, ProviderError> {
        Ok(format!("tail -f {}/{}.log", self.tmp_dir, name))
    }

    // TODO: Add test
    async fn pause(&self, node_name: &str) -> Result<(), ProviderError> {
        let process = self
            .process_map
            .get(node_name)
            .ok_or(ProviderError::MissingNodeInfo(
                node_name.to_owned(),
                "process".into(),
            ))?;

        let _ = self
            .run_command(
                vec![format!("kill -STOP {}", process.pid)],
                NativeRunCommandOptions {
                    is_failure_allowed: true,
                },
            )
            .await?;
        Ok(())
    }

    // TODO: Add test
    async fn resume(&self, node_name: &str) -> Result<(), ProviderError> {
        let process = self
            .process_map
            .get(node_name)
            .ok_or(ProviderError::MissingNodeInfo(
                node_name.to_owned(),
                "process".into(),
            ))?;

        let _ = self
            .run_command(
                vec![format!("kill -CONT {}", process.pid)],
                NativeRunCommandOptions {
                    is_failure_allowed: true,
                },
            )
            .await?;
        Ok(())
    }

    // TODO: Add test
    async fn restart(&mut self, node_name: &str, _after_secs: u16) -> Result<bool, ProviderError> {
        let pid = self
            .process_map
            .get(node_name)
            .ok_or(ProviderError::MissingNodeInfo(
                node_name.to_owned(),
                "process".into(),
            ))?;
        self.run_command(
            [format!("kill -9 {:?}", pid)].to_vec(),
            NativeRunCommandOptions {
                is_failure_allowed: true,
            },
        )
        .await?;

        let Some(process) = self.process_map.get_mut(node_name) else {
          return Err(ProviderError::MissingNodeInfo(
                      node_name.to_owned(),
                      "process".into(),));
      };

        let mapped_env: HashMap<String, String> = env::vars().map(|(k, v)| (k, v)).collect();

        let res = Command::new(self.command.clone())
            .arg("-c")
            .envs(&mapped_env)
            .spawn();

        let restarted_process = match res {
            Ok(process) => process,
            Err(e) => return Err(ProviderError::ErrorSpawningNode(e.to_string())),
        };

        process.pid = restarted_process.id().unwrap();

        Ok(true)
    }

    async fn get_node_info(&self, node_name: &str) -> Result<(IpAddr, Port), ProviderError> {
        let host_port = self.get_port_mapping(P2P_PORT, node_name).await?;
        Ok((LOCALHOST, host_port))
    }

    async fn get_node_ip(&self, _node_name: &str) -> Result<IpAddr, ProviderError> {
        Ok(LOCALHOST)
    }

    async fn get_port_mapping(&self, port: Port, node_name: &str) -> Result<Port, ProviderError> {
        let r = match self.process_map.get(node_name) {
            Some(process) => match process.port_mapping.get(&port) {
                Some(port) => Ok(*port),
                None => Err(ProviderError::MissingNodeInfo(
                    node_name.to_owned(),
                    "port".into(),
                )),
            },
            None => Err(ProviderError::MissingNodeInfo(
                node_name.to_owned(),
                "process".into(),
            )),
        };

        return r;
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use support::fs::mock::{MockError, MockFilesystem, Operation};

    use super::*;

    #[test]
    fn new_native_provider() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        assert_eq!(native_provider.namespace, "something");
        assert_eq!(native_provider.config_path, "./");
        assert!(native_provider.is_debug);
        assert_eq!(native_provider.timeout, 60);
        assert_eq!(native_provider.tmp_dir, "/tmp");
        assert_eq!(native_provider.command, "bash");
        assert_eq!(native_provider.local_magic_file_path, "/tmp/finished.txt");
        assert_eq!(native_provider.remote_dir, "/tmp/cfg");
        assert_eq!(native_provider.data_dir, "/tmp/data");
    }

    #[tokio::test]
    async fn test_fielsystem_usage() {
        let mut native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        native_provider.create_namespace().await.unwrap();

        assert!(native_provider.filesystem.operations.len() == 1);

        assert_eq!(
            native_provider.filesystem.operations[0],
            Operation::CreateDir {
                path: "/tmp/cfg".into(),
            }
        );
    }

    #[tokio::test]
    #[should_panic(expected = "FSError(OpError(\"create\"))")]
    async fn test_fielsystem_usage_fails() {
        let mut native_provider: NativeProvider<MockFilesystem> = NativeProvider::new(
            "something",
            "./",
            "/tmp",
            MockFilesystem::with_create_dir_error(MockError::OpError("create".into())),
        );

        native_provider.create_namespace().await.unwrap();
    }

    #[tokio::test]
    async fn test_get_node_ip() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        assert_eq!(
            native_provider.get_node_ip("some").await.unwrap(),
            LOCALHOST
        );
    }

    #[tokio::test]
    async fn test_run_command_when_bash_is_removed() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        let result: RunCommandResponse = native_provider
            .run_command(
                vec!["bash".into(), "ls".into()],
                NativeRunCommandOptions::default(),
            )
            .await
            .unwrap();

        assert_eq!(
            result,
            RunCommandResponse {
                exit_code: ExitStatus::from_raw(0),
                std_out: "Cargo.toml\nsrc\n".into(),
                std_err: None,
            }
        );
    }

    #[tokio::test]
    async fn test_run_command_when_dash_c_is_provided() {
        let native_provider = NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        let result = native_provider.run_command(
            vec!["-c".into(), "ls".into()],
            NativeRunCommandOptions::default(),
        );

        let a = result.await;
        assert!(a.is_ok());
    }

    #[tokio::test]
    async fn test_run_command_when_error_return_error() {
        let native_provider = NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        let mut some = native_provider.run_command(
            vec!["ls".into(), "ls".into()],
            NativeRunCommandOptions::default(),
        );

        assert!(some.await.is_err());

        some = native_provider.run_command(
            vec!["ls".into(), "ls".into()],
            NativeRunCommandOptions {
                is_failure_allowed: true,
            },
        );

        assert!(some.await.is_ok());
    }
}
