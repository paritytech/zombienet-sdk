use std::{self, collections::HashMap, collections::hash_map::Entry::{Occupied, Vacant}, error::Error, fmt::Debug, io::ErrorKind, process::Stdio};


use async_trait::async_trait;
use serde::Serialize;
use tokio::{
    process::{Child, Command},
    time::{sleep, Duration},
};

use super::Provider;

use crate::shared::{
    constants::{DEFAULT_DATA_DIR, DEFAULT_REMOTE_DIR, LOCALHOST, P2P_PORT},
    types::{NativeRunCommandOptions, PodDef, Process, RunCommandResponse, ZombieRole},
};

use crate::errors::ProviderError;
use support::fs::FileSystem;

#[derive(Debug, Serialize, Clone, PartialEq)]
struct NativeProvider<T: FileSystem + Send + Sync> {
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
    process_map:              HashMap<String, Process>,
    filesystem:               T,
}

impl<T: FileSystem + Send + Sync> NativeProvider<T> {
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
            is_pod_monitor_available: false,
            process_map,
            filesystem,
        }
    }
}

#[async_trait]
impl<T: FileSystem + Send + Sync> Provider for NativeProvider<T> {
    async fn create_namespace(&mut self) -> Result<(), Box<dyn Error>> {
        // Native provider don't have the `namespace` isolation.
        // but we create the `remoteDir` to place files
        self.filesystem.create_dir(&self.remote_dir)?;
        Ok(())
    }

    async fn get_port_mapping(&mut self, port: u16, pod_name: String) -> Result<u16, Box<dyn Error>> {
        match self.process_map.get(&pod_name) {
            Some(process) => {
                match process.port_mapping.get(&port) {
                    Some(port) => return Ok(*port),
                    // TODO: return specialized error
                    None => {},
                }
            },
            // TODO: return specialized error
            None => {}
        };

        Err(Box::new(ProviderError::TodoErr))
    }

    async fn get_node_info(&mut self, pod_name: String) -> Result<(String, u16), Box<dyn Error>> {
        let host_port = self.get_port_mapping(P2P_PORT, pod_name).await?;
        Ok((LOCALHOST.to_string(), host_port))
    }

    async fn get_node_ip(&self) -> Result<String, Box<dyn Error>> {
        Ok(LOCALHOST.to_owned())
    }

    async fn run_command(
        &self,
        mut args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, Box<dyn Error>> {
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

        let result = Command::new("sh")
            .arg("-c")
            .arg(args.join(" "))
            .output()
            .await?;


        if !result.status.success() && !opts.allow_fail {
            return Err(Box::new(std::io::Error::new(
                ErrorKind::Other,
                "Allow fail",
            )));

        } else {
            // cmd success or we allow to fail
            // in either case we return Ok
            Ok(RunCommandResponse {
                exit_code: result.status,
                std_out:   result.stdout,
                std_err:   if result.stderr.is_empty() { None } else {Some(result.stderr)},
            })
        }
    }

    async fn create_resource(&mut self, resource_def: PodDef) -> Result<(), Box<dyn Error>> {
        let name: String = resource_def.metadata.name.clone();
        let local_file_path: String = format!("{}/{}.yaml", &self.tmp_dir, name);
        let content: String = serde_json::to_string(&resource_def)?;

        self.filesystem
            .write(&local_file_path, content)?;

        let command = if resource_def.spec.command.starts_with("bash") {
            resource_def.spec.command.replace("bash", "")
        } else {
            resource_def.spec.command
        };

        if resource_def.metadata.labels.zombie_role == ZombieRole::Temp {
            // for temp we run some short living cmds
            self.run_command(
                vec![command],
                NativeRunCommandOptions {
                    allow_fail: Some(true).is_some(),
                },
            )
            .await?;
        } else {
            // Allo others are spawned.
            let child_process: Child = match Command::new("sh")
                .arg("-c")
                .arg(command.clone())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Err(why) => panic!("Couldn't spawn process: {}", why),
                Ok(node_process) => node_process,
            };

            // TODO: log::debug!(node_process.id());
            //   nodeProcess.stdout.pipe(log);
            //   nodeProcess.stderr.pipe(log);

            match self.process_map.entry(name.clone()) {
                // TODO: return specific error
                Occupied(_) => return Err(Box::new(ProviderError::TodoErr)),
                Vacant(slot) => {
                    slot.insert(Process {
                        pid: child_process.id().unwrap(),
                        //TODO: complete this field
                        log_dir: "".into(),
                        //TODO: complete this field
                        port_mapping: HashMap::default(),
                        command: command,
                    });
                }
            }

            // TODO: uncomment when resolve the spawn method
            //let _ = self.wait_node_ready(name).await;

        }
        Ok(())
    }

    async fn destroy_namespace(&mut self) -> Result<(), Box<dyn Error>> {
        // get pod names
        let mut memo: Vec<String> = Vec::new();
        let pids: Vec<String> = self
            .process_map
            .iter()
            .filter(|(_, process)| process.pid != 0)
            .map(|(_, process)| {
                memo.push(process.pid.to_string());
                process.pid.to_string()
            })
            .collect();

        let result = self
            .run_command(
                [format!(
                    "ps ax| awk '{{print $1}}'| grep -E '{}'",
                    pids.join("|")
                )]
                .to_vec(),
                NativeRunCommandOptions { allow_fail: true },
            )
            .await
            .unwrap();

        if result.exit_code.code().unwrap() == 0 {
            let pids_to_kill: Vec<String> = result
                .std_out
                .split(|c| c == &b'\n')
                .map(|s| String::from_utf8(s.to_vec()).unwrap())
                .collect();

            self.run_command(
                [format!("kill -9 {}", pids_to_kill.join(" "))].to_vec(),
                NativeRunCommandOptions { allow_fail: true },
            )
            .await
            .expect("Failed to kill process");
        }
        Ok(())
    }

    async fn get_node_logs(&mut self, name: String) -> Result<String, Box<dyn Error>> {
        // For now in native let's just return all the logs
        let result: Result<String, Box<dyn Error>> = self
            .filesystem
            .read_file(&format!("{}/{}.log", self.tmp_dir, name));
        return result;
    }

    async fn dump_logs(&mut self, path: String, pod_name: String) -> Result<(), Box<dyn Error>> {
        let dst_file_name: String = format!("{}/logs/{}.log", path, pod_name);
        self.filesystem
            .copy(
                &format!("{}/{}.log", self.tmp_dir, pod_name),
                &dst_file_name,
            )
            .expect("Failed to copy file");
        Ok(())
    }

    async fn wait_node_ready(&mut self, node_name: String) -> Result<(), Box<dyn Error>> {
        // check if the process is alive after 1 seconds
        sleep(Duration::from_millis(1000)).await;

        let process_node_name = self.process_map.get_mut(&node_name).unwrap();

        let pid = process_node_name.pid;
        let log_dir = process_node_name.log_dir.clone();

        let result = self
            .run_command(
                vec![format!("ps {}", pid)],
                NativeRunCommandOptions { allow_fail: true },
            )
            .await
            .expect("Failed to run `ps` command");

        if result.exit_code.code().unwrap() > 0 {
            let lines: String = self.get_node_logs(node_name).await?;
            // Javier - TODO: check how we will log with tables
            // TODO: Log with a log table
            // const logTable = new CreateLogTable({
            //   colWidths: [20, 100],
            // });
            // logTable.pushToPrint([
            //   [decorators.cyan("Pod"), decorators.green(nodeName)],
            //   [
            //     decorators.cyan("Status"),
            //     decorators.reverse(decorators.red("Error")),
            //   ],
            //   [
            //     decorators.cyan("Message"),
            //     decorators.white(`Process: ${pid}, for node: ${nodeName} dies.`),
            //   ],
            //   [decorators.cyan("Output"), decorators.white(lines)],
            // ]);

            return Err(Box::new(std::io::Error::new(
                ErrorKind::Other,
                "An error occured",
            )));
        }

        // check log lines grow between 2/6/12 secs
        let lines_1: RunCommandResponse = self
            .run_command(
                vec![format!("wc -l  {}", log_dir)],
                NativeRunCommandOptions::default(),
            )
            .await
            .expect("Failed to run `wc -l` command");
        sleep(Duration::from_millis(2000)).await;

        let lines_2: RunCommandResponse = self
            .run_command(
                vec![format!("wc -l  {}", log_dir)],
                NativeRunCommandOptions::default(),
            )
            .await
            .expect("Failed to run `wc -l` command");

        // Javier-TODO: This looks weird and wrong
        let lines_1_output: u32 = String::from_utf8(lines_1.std_out)
            .unwrap()
            .parse::<u32>()
            .expect("Error while converting 1st time, lines, to u32");
        let lines_2_output: u32 = String::from_utf8(lines_2.std_out)
            .unwrap()
            .parse::<u32>()
            .expect("Error while converting 2nd time, lines, to u32");

        if lines_2_output > lines_1_output {
            return Ok(());
        };
        sleep(Duration::from_millis(6000)).await;

        let lines_3: RunCommandResponse = self
            .run_command(
                vec![format!("wc -l  {}", log_dir)],
                NativeRunCommandOptions::default(),
            )
            .await
            .expect("Failed to run `wc -l` command");

        let lines_3_output: u32 = String::from_utf8(lines_3.std_out)
            .unwrap()
            .parse::<u32>()
            .expect("Error while converting 3rd time, lines, to u32");

        if lines_3_output > lines_1_output {
            return Ok(());
        };
        sleep(Duration::from_millis(12000)).await;

        let lines_4: RunCommandResponse = self
            .run_command(
                vec![format!("wc -l  {}", log_dir)],
                NativeRunCommandOptions::default(),
            )
            .await
            .expect("Failed to run `wc -l` command");

        let lines_4_output: u32 = String::from_utf8(lines_4.std_out)
            .unwrap()
            .parse::<u32>()
            .expect("Error while converting 4th time, lines, to u32");

        if lines_4_output > lines_1_output {
            return Ok(());
        };

        let error_string = format!(
            "Log lines of process: {} ( node: {} ) doesn't grow, please check logs at {}",
            pid, node_name, log_dir
        );

        return Err(Box::new(std::io::Error::new(
            ErrorKind::Other,
            error_string,
        )));
    }

    fn get_pause_args(&mut self, name: String) -> Vec<String> {
        let command = format!("kill -STOP {}", self.process_map[&name].pid);
        [command].to_vec()
    }

    fn get_resume_args(&mut self, name: String) -> Vec<String> {
        let command = format!("kill -CONT {}", self.process_map[&name].pid);
        [command].to_vec()
    }

    async fn validate_access(&mut self) -> Result<bool, Box<dyn Error>> {
        let result = self
            .run_command(
                vec!["--help".to_owned()],
                NativeRunCommandOptions::default(),
            )
            .await
            .expect("Failed to run `--help` command");
        Ok(result.exit_code.code().unwrap() == 0)
    }
}

// Javier-TODO: File Testings (copy etc etc)
#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use super::*;
    use support::fs::mock::{MockFilesystem,Operation};
    use crate::{
        shared::types::{PodLabels, PodMetadata, PodSpec},
    };

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

    #[tokio::test]
    async fn test_fielsystem_usage() {
        let mut native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let _ = native_provider.create_namespace().await;

        assert!(native_provider.filesystem.operations.len() == 1);

        assert_eq!(
            native_provider.filesystem.operations[0],
            Operation::CreateDir {
                path: "./tmp/cfg".into(),
            }
        );
    }

    #[tokio::test]
    async fn test_get_node_ip() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        assert_eq!(native_provider.get_node_ip().await.unwrap(), LOCALHOST);
    }

    #[tokio::test]
    async fn test_run_command_when_bash_is_removed() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let result: RunCommandResponse = native_provider
            .run_command(
                vec!["bash".into(), "ls".into()],
                NativeRunCommandOptions::default(),
            )
            .await
            .expect("Error");

        assert_eq!(
            result,
            RunCommandResponse {
                exit_code: ExitStatus::from_raw(0),
                std_out:   "Cargo.toml\nsrc\n".into(),
                std_err:   None,
            }
        );
    }

    #[tokio::test]
    async fn test_run_command_when_dash_c_is_provided() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let result = native_provider.run_command(
            vec!["-c".into(), "ls".into()],
            NativeRunCommandOptions::default(),
        );

        let a = result.await;
        assert!(a.is_ok());
    }

    #[tokio::test]
    async fn test_run_command_when_error_return_error() {
        let native_provider =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let mut some = native_provider.run_command(
            vec!["ls".into(), "ls".into()],
            NativeRunCommandOptions::default(),
        );

        assert!(some.await.is_err());

        some = native_provider.run_command(
            vec!["ls".into(), "ls".into()],
            NativeRunCommandOptions { allow_fail: true },
        );

        assert!(some.await.is_ok());
    }

    #[tokio::test]
    async fn test_create_resource() {
        let mut native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "./tmp", MockFilesystem::new());

        let mut env = std::collections::HashMap::new();
        env.insert("SOME".to_owned(), "VALUE".to_owned());

        let resource_def: PodDef = PodDef {
            metadata: PodMetadata {
                name:      "string".to_owned(),
                namespace: "string".to_owned(),
                labels:    PodLabels {
                    app:         "String".to_owned(),
                    zombie_ns:   "String".to_owned(),
                    name:        "String".to_owned(),
                    instance:    "String".to_owned(),
                    zombie_role: ZombieRole::Node,
                },
            },
            spec:     PodSpec {
                cfg_path: "string".to_owned(),
                data_path: "string".to_owned(),
                ports: vec![],
                command: "ls".to_owned(),
                env,
            },
        };

        native_provider
            .create_resource(resource_def)
            .await
            .expect("err");

        assert_eq!(native_provider.process_map.len(), 1);
    }
}
