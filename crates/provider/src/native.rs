use std::{
    self,
    collections::{
        hash_map::Entry::{Occupied, Vacant},
        HashMap,
    },
    error::Error,
    fmt::Debug,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
};

use async_trait::async_trait;
use serde::Serialize;
use support::fs::FileSystem;
use tokio::{
    process::{Child, Command},
    time::{sleep, Duration},
};

use super::Provider;
use crate::{
    errors::ProviderError,
    shared::{
        constants::{DEFAULT_DATA_DIR, DEFAULT_REMOTE_DIR, LOCALHOST, P2P_PORT},
        types::{
            FileMap, NativeRunCommandOptions, PodDef, Process, RunCommandResponse, ZombieRole,
        },
    },
};

#[derive(Debug, Serialize, Clone, PartialEq)]
pub(crate) struct NativeProvider<T: FileSystem + Send + Sync> {
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
    async fn create_namespace(&mut self) -> Result<(), ProviderError> {
        // Native provider don't have the `namespace` isolation.
        // but we create the `remoteDir` to place files
        self.filesystem
            .create_dir(&self.remote_dir)
            .await
            .map_err(|e| ProviderError::FSError(Box::new(e)))?;
        Ok(())
    }

    async fn get_port_mapping(
        &mut self,
        port: u16,
        pod_name: String,
    ) -> Result<u16, ProviderError> {
        let r = match self.process_map.get(&pod_name) {
            Some(process) => {
                match process.port_mapping.get(&port) {
                    Some(port) => Ok(*port),
                    // TODO: return specialized error
                    None => Err(ProviderError::MissingNodeInfo(pod_name, "port".into())),
                }
            },
            // TODO: return specialized error
            None => Err(ProviderError::MissingNodeInfo(pod_name, "process".into())),
        };

        return r;
    }

    async fn get_node_info(&mut self, pod_name: String) -> Result<(String, u16), ProviderError> {
        let host_port = self.get_port_mapping(P2P_PORT, pod_name).await?;
        Ok((LOCALHOST.to_string(), host_port))
    }

    async fn get_node_ip(&self) -> Result<String, ProviderError> {
        Ok(LOCALHOST.to_owned())
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

        let result = Command::new("sh")
            .arg("-c")
            .arg(args.join(" "))
            .output()
            .await?;

        if !result.status.success() && !opts.allow_fail {
            return Err(ProviderError::RunCommandError(args.join(" ")));
        } else {
            // cmd success or we allow to fail
            // in either case we return Ok
            Ok(RunCommandResponse {
                exit_code: result.status,
                std_out:   String::from_utf8_lossy(&result.stdout).into(),
                std_err:   if result.stderr.is_empty() {
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
        let script_filename: &str = Path::new(&script_path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let script_path_in_pod: String =
            format!("{}/{}/{}", self.tmp_dir, identifier, script_filename);

        // upload the script
        let _ = self
            .filesystem
            .copy(&script_path, &script_path_in_pod)
            .await;

        // set as executable
        self.run_command(
            vec![
                "chmod".to_owned(),
                "+x".to_owned(),
                script_path_in_pod.to_owned(),
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
            std_out:   result.std_out,
            std_err:   result.std_err,
        })
    }

    async fn spawn_from_def(
        &mut self,
        pod_def: PodDef,
        files_to_copy: Vec<FileMap>,
        keystore: String,
        chain_spec_id: String,
        db_snapshot: String,
    ) -> Result<(), ProviderError> {
        let name = pod_def.metadata.name.clone();
        // TODO: log::debug!(format!("{}", serde_json::to_string(&pod_def)));

        // keep this in the client.
        self.process_map.entry(name.clone()).and_modify(|p| {
            p.logs = format!("{}/{}.log", self.tmp_dir, name);
            p.port_mapping = pod_def
                .spec
                .ports
                .iter()
                .map(|item| (item.container_port, item.host_port))
                .collect();
        });

        // TODO: check how we will log with tables
        // let logTable = new CreateLogTable({
        //   colWidths: [25, 100],
        // });

        // const logs = [
        //   [decorators.cyan("Pod"), decorators.green(name)],
        //   [decorators.cyan("Status"), decorators.green("Launching")],
        //   [
        //     decorators.cyan("Command"),
        //     decorators.white(podDef.spec.command.join(" ")),
        //   ],
        // ];
        // if (dbSnapshot) {
        //   logs.push([decorators.cyan("DB Snapshot"), decorators.green(dbSnapshot)]);
        // }
        // logTable.pushToPrint(logs);

        // we need to get the snapshot from a public access
        // and extract to /data
        let _ = self
            .filesystem
            .create_dir(format!("{}", pod_def.spec.data_path))
            .await;

        // TODO: await downloadFile(dbSnapshot, `${podDef.spec.dataPath}/db.tgz`);
        let command = format!("cd {}/.. && tar -xzvf data/db.tgz", pod_def.spec.data_path);

        self.run_command(vec![command], NativeRunCommandOptions::default())
            .await?;

        if !keystore.is_empty() {
            // initialize keystore
            let keystore_remote_dir = format!(
                "{}/chains/{}/keystore",
                pod_def.spec.data_path, chain_spec_id
            );

            let _ = self
                .filesystem
                .create_dir(format!("{}", keystore_remote_dir))
                .await;

            let _ = self.filesystem.copy(&keystore, &keystore_remote_dir).await;
        }

        let files_to_copy_iter = files_to_copy.iter();

        for file in files_to_copy_iter {
            // log::debug!(format!("file.local_file_path: {}", file.local_file_path));
            // log::debug!(format!("file.remote_file_path: {}", file.remote_file_path));

            // log::debug!(format!("self.remote_dir: {}", self.remote_dir);
            // log::debug!(format!("self.data_dir: {}", self.data_dir);

            let remote_file_path_str: String = file
                .clone()
                .remote_file_path
                .into_os_string()
                .into_string()
                .unwrap();

            let mut resolved_remote_file_path = String::new();
            if remote_file_path_str.contains(&self.remote_dir) {
                resolved_remote_file_path = format!(
                    "{}/{}",
                    &pod_def.spec.cfg_path,
                    remote_file_path_str.replace(&self.remote_dir, "")
                );
            } else {
                resolved_remote_file_path = format!(
                    "{}/{}",
                    &pod_def.spec.data_path,
                    remote_file_path_str.replace(&self.data_dir, "")
                );
            }

            let _ = self
                .filesystem
                .copy(
                    file.clone()
                        .local_file_path
                        .into_os_string()
                        .into_string()
                        .unwrap(),
                    resolved_remote_file_path,
                )
                .await;
        }

        self.create_resource(pod_def, false, true).await?;

        // TODO: check how we will log with tables
        // logTable = new CreateLogTable({
        //   colWidths: [40, 80],
        // });
        // logTable.pushToPrint([
        //   [decorators.cyan("Pod"), decorators.green(name)],
        //   [decorators.cyan("Status"), decorators.green("Ready")],
        // ]);
        Ok(())
    }

    async fn copy_file_from_pod(
        &mut self,
        pod_file_path: PathBuf,
        local_file_path: PathBuf,
    ) -> Result<(), ProviderError> {
        // TODO: log::debug!(format!("cp {} {}", pod_file_path, local_file_path));

        let _ = self.filesystem.copy(&pod_file_path, &local_file_path).await;
        Ok(())
    }

    async fn create_resource(
        &mut self,
        mut resource_def: PodDef,
        _scoped: bool,
        wait_ready: bool,
    ) -> Result<(), ProviderError> {
        let name: String = resource_def.metadata.name.clone();
        let local_file_path: String = format!("{}/{}.yaml", &self.tmp_dir, name);
        let content: String = serde_json::to_string(&resource_def)?;

        self.filesystem
            .write(&local_file_path, content)
            .await
            .map_err(|e| ProviderError::FSError(Box::new(e)))?;

        if resource_def.spec.command.get(0) == Some(&"bash".into()) {
            resource_def.spec.command.remove(0);
        }

        if resource_def.metadata.labels.zombie_role == ZombieRole::Temp {
            // for temp we run some short living cmds
            self.run_command(
                resource_def.spec.command,
                NativeRunCommandOptions {
                    allow_fail: Some(true).is_some(),
                },
            )
            .await?;
        } else {
            // Allow others are spawned.
            let logs = format!("{}/{}.log", self.tmp_dir, name);
            let file_handler = self
                .filesystem
                .create(logs.clone())
                .await
                .map_err(|e| ProviderError::FSError(Box::new(e)))?;
            let final_command = resource_def.spec.command.join(" ");

            let child_process = std::process::Command::new(&self.command)
                .arg("-c")
                .arg(final_command.clone())
                // TODO: set env
                .stdout(file_handler)
                // TODO: redirect stderr to the same stdout
                //.stderr()
                .spawn()?;
            // {
            //     Err(why) => panic!("Couldn't spawn process: {}", why),
            //     Ok(node_process) => node_process,
            // };

            // TODO: log::debug!(node_process.id());
            //   nodeProcess.stdout.pipe(log);
            //   nodeProcess.stderr.pipe(log);

            match self.process_map.entry(name.clone()) {
                // TODO: return specific error
                Occupied(_) => return Err(ProviderError::DuplicatedNodeName(name)),
                Vacant(slot) => {
                    slot.insert(Process {
                        pid: child_process.id(),
                        // TODO: complete this field
                        logs,
                        // TODO: complete this field
                        port_mapping: HashMap::default(),
                        command: final_command,
                    });
                },
            }

            // logs: `${this.tmpDir}/${name}.log`,
            // portMapping: podDef.spec.ports.reduce((memo: any, item: any) => {
            //   memo[item.containerPort] = item.hostPort;
            //   return memo;
            // }, {}),

            if wait_ready {
                self.wait_node_ready(name).await?;
            }
        }
        Ok(())
    }

    // TODO: Add test
    async fn destroy_namespace(&mut self) -> Result<(), ProviderError> {
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
                .split(|c| c == '\n')
                .map(|s| s.into())
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

    // TODO: Add test
    async fn get_node_logs(&mut self, name: String) -> Result<String, ProviderError> {
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

    async fn wait_node_ready(&mut self, node_name: String) -> Result<(), ProviderError> {
        // check if the process is alive after 1 seconds
        sleep(Duration::from_millis(1000)).await;

        let Some(process_node) = self.process_map.get(&node_name) else {
            return Err(ProviderError::MissingNodeInfo(node_name, "process".into()));
        };

        let result = self
            .run_command(
                vec![format!("ps {}", process_node.pid)],
                NativeRunCommandOptions { allow_fail: true },
            )
            .await?;

        if result.exit_code.code().unwrap() > 0 {
            let lines: String = self.get_node_logs(node_name).await?;
            // TODO: check how we will log with tables
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

            return Err(ProviderError::NodeNotReady(lines));
        }

        // Process pid is
        // check log lines grow between 2/6/12 secs
        let lines_intial: RunCommandResponse = self
            .run_command(
                vec![format!("wc -l  {}", process_node.logs)],
                NativeRunCommandOptions::default(),
            )
            .await?;

        for i in [2000, 6000, 12000] {
            sleep(Duration::from_millis(i)).await;
            let lines_now = self
                .run_command(
                    vec![format!("wc -l  {}", process_node.logs)],
                    NativeRunCommandOptions::default(),
                )
                .await?;
            if lines_now.std_out > lines_intial.std_out {
                return Ok(());
            };
        }

        let error_string = format!(
            "Log lines of process: {} ( node: {} ) doesn't grow, please check logs at {}",
            process_node.pid, node_name, process_node.logs
        );

        Err(ProviderError::NodeNotReady(error_string))
    }

    // TODO: Add test
    fn get_pause_args(&mut self, name: String) -> Vec<String> {
        let command = format!("kill -STOP {}", self.process_map[&name].pid);
        [command].to_vec()
    }

    // TODO: Add test
    fn get_resume_args(&mut self, name: String) -> Vec<String> {
        let command = format!("kill -CONT {}", self.process_map[&name].pid);
        [command].to_vec()
    }

    async fn restart_node(&mut self, name: String, timeout: u64) -> Result<bool, ProviderError> {
        let command = format!("kill -9 {}", self.process_map[&name].pid);
        let result = self
            .run_command(vec![command], NativeRunCommandOptions { allow_fail: true })
            .await?;

        if result.exit_code.code().unwrap() > 0 {
            return Ok(false);
        }

        sleep(Duration::from_millis(timeout * 1000)).await;

        Ok(true)
    }

    //   // start
    //   const log = fs.createWriteStream(this.processMap[name].logs);
    //   console.log(["-c", ...this.processMap[name].cmd!]);
      const nodeProcess = spawn(this.command, [
        "-c",
        ...this.processMap[name].cmd!,
      ]);

      let file_handler = self
      .filesystem
      .create(logs.clone())
      .await
      .map_err(|e| ProviderError::FSError(Box::new(e)))?;
  let final_command = resource_def.spec.command.join(" ");

  let child_process = std::process::Command::new(&self.command)
      .arg("-c")
      .arg(final_command.clone())
      // TODO: set env
      .stdout(file_handler)
      // TODO: redirect stderr to the same stdout
      //.stderr()
      .spawn()?;
    //   debug(nodeProcess.pid);
    //   nodeProcess.stdout.pipe(log);
    //   nodeProcess.stderr.pipe(log);
    //   this.processMap[name].pid = nodeProcess.pid;

    //   await this.wait_node_ready(name);
    //   return true;
    // }

    // getLogsCommand(name: string): string {
    //   return `tail -f  ${this.tmpDir}/${name}.log`;
    // }

    // TODO: Add test
    async fn validate_access(&mut self) -> Result<bool, ProviderError> {
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

    use support::fs::mock::{MockFilesystem, Operation};

    use super::*;
    use crate::shared::types::{PodLabels, PodMetadata, PodSpec};

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
        assert!(!native_provider.is_pod_monitor_available);
        assert_eq!(native_provider.local_magic_file_path, "/tmp/finished.txt");
        assert_eq!(native_provider.remote_dir, "/tmp/cfg");
        assert_eq!(native_provider.data_dir, "/tmp/data");
    }

    #[tokio::test]
    async fn test_fielsystem_usage() {
        let mut native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        let _ = native_provider.create_namespace().await;

        assert!(native_provider.filesystem.operations.len() == 1);

        assert_eq!(
            native_provider.filesystem.operations[0],
            Operation::CreateDir {
                path: "/tmp/cfg".into(),
            }
        );
    }

    #[tokio::test]
    async fn test_get_node_ip() {
        let native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        assert_eq!(native_provider.get_node_ip().await.unwrap(), LOCALHOST);
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
            NativeRunCommandOptions { allow_fail: true },
        );

        assert!(some.await.is_ok());
    }

    #[tokio::test]
    async fn test_create_resource() {
        let mut native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

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
                command: vec!["ls".to_owned()],
                env,
            },
        };

        native_provider
            .create_resource(resource_def, false, false)
            .await
            .expect("err");

        assert_eq!(native_provider.process_map.len(), 1);
    }
    #[tokio::test]
    async fn test_create_resource_wait_ready() {
        let mut native_provider: NativeProvider<MockFilesystem> =
            NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

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
                command: vec!["for i in $(seq 1 10); do echo $i;sleep 1;done".into()],
                env,
            },
        };

        native_provider
            .create_resource(resource_def, false, true)
            .await
            .expect("err");

        assert_eq!(native_provider.process_map.len(), 1);
    }
}
