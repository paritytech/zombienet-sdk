use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::shared::resources::Resources;
use futures::future::try_join_all;
use support::fs::FileSystem;
use tokio::{time::sleep, try_join};

use super::{namespace::KubernetesNamespace, pod_spec_builder::PodSpecBuilder};
use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, NODE_SCRIPTS_DIR},
    types::{ExecutionResult, RunCommandOptions, RunScriptOptions, TransferedFile},
    KubernetesClient, ProviderError, ProviderNamespace, ProviderNode,
};

pub(super) struct KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    namespace: Weak<KubernetesNamespace<FS>>,
    name: String,
    args: Vec<String>,
    base_dir: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    relay_data_dir: PathBuf,
    scripts_dir: PathBuf,
    log_path: PathBuf,
    k8s_client: KubernetesClient,
    http_client: reqwest::Client,
    filesystem: FS,
}

impl<FS> KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        namespace: &Weak<KubernetesNamespace<FS>>,
        namespace_base_dir: &PathBuf,
        name: &str,
        image: Option<&String>,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        startup_files: &[TransferedFile],
        resources: Option<&Resources>,
        k8s_client: &KubernetesClient,
        filesystem: &FS,
    ) -> Result<Arc<Self>, ProviderError> {
        let base_dir = PathBuf::from_iter([&namespace_base_dir, &PathBuf::from(name)]);
        filesystem.create_dir_all(&base_dir).await?;

        let base_dir_raw = base_dir.to_string_lossy();
        let config_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_DATA_DIR));
        let relay_data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_RELAY_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_SCRIPTS_DIR));
        let log_path = base_dir.join("node.log");

        try_join!(
            filesystem.create_dir(&config_dir),
            filesystem.create_dir(&data_dir),
            filesystem.create_dir(&relay_data_dir),
            filesystem.create_dir(&scripts_dir),
        )?;

        let node = Arc::new(KubernetesNode {
            namespace: namespace.clone(),
            name: name.to_string(),
            args: args.to_vec(),
            base_dir,
            config_dir,
            data_dir,
            relay_data_dir,
            scripts_dir,
            log_path,
            filesystem: filesystem.clone(),
            k8s_client: k8s_client.clone(),
            http_client: reqwest::Client::new(),
        });

        node.initialize_k8s(image, program, args, env, resources)
            .await?;

        node.initialize_startup_files(startup_files).await?;

        node.start().await?;

        Ok(node)
    }

    async fn initialize_k8s(
        &self,
        image: Option<&String>,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        resources: Option<&Resources>,
    ) -> Result<(), ProviderError> {
        let labels = BTreeMap::from([("foo".to_string(), "bar".to_string())]);
        let image = image.ok_or_else(|| {
            ProviderError::MissingNodeInfo(self.name.to_string(), "missing image".to_string())
        })?;

        let pod_spec = PodSpecBuilder::build(&self.name, image, resources, program, args, env);

        let manifest = self
            .k8s_client
            .create_pod(&self.namespace_name(), &self.name, pod_spec, labels)
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.clone(), err.into()))?;

        let serialized_manifest = serde_yaml::to_string(&manifest)
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;

        let dest_path = PathBuf::from_iter([
            &self.base_dir,
            &PathBuf::from(format!("{}_manifest.yaml", &self.name)),
        ]);

        self.filesystem
            .write(dest_path, serialized_manifest)
            .await
            .map_err(|err| ProviderError::NodeSpawningFailed(self.name.to_string(), err.into()))?;

        Ok(())
    }

    async fn initialize_startup_files(
        &self,
        startup_files: &[TransferedFile],
    ) -> Result<(), ProviderError> {
        try_join_all(
            startup_files
                .iter()
                .map(|file| self.send_file(&file.local_path, &file.remote_path, &file.mode)),
        )
        .await?;

        Ok(())
    }

    pub(super) async fn start(&self) -> Result<(), ProviderError> {
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", "echo start > /tmp/zombiepipe"],
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to start pod {} after spawning", self.name),
                    err.into(),
                )
            })?
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to start pod {} after spawning", self.name,),
                    anyhow!("command failed in container: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    fn get_remote_parent_dir(&self, remote_file_path: &Path) -> Option<PathBuf> {
        if let Some(remote_parent_dir) = remote_file_path.parent() {
            if matches!(
                remote_parent_dir.components().rev().peekable().peek(),
                Some(Component::Normal(_))
            ) {
                return Some(remote_parent_dir.to_path_buf());
            }
        }

        None
    }

    async fn create_remote_dir(&self, remote_dir: &Path) -> Result<(), ProviderError> {
        let _ = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["mkdir", "-p", &remote_dir.to_string_lossy()],
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to created dirfor pod {}", &self.name),
                    err.into(),
                )
            })?;

        Ok(())
    }

    fn namespace_name(&self) -> String {
        self.namespace
            .upgrade()
            .map(|namespace| namespace.name().to_string())
            .expect("namespace shouldn't be dropped")
    }

    async fn file_server_local_host(&self) -> Result<String, ProviderError> {
        if let Some(namespace) = self.namespace.upgrade() {
            if let Some(port) = *namespace.file_server_port.read().await {
                return Ok(format!("localhost:{port}"));
            }
        }

        Err(ProviderError::FileServerSetupError(anyhow!(
            "file server port not bound locally"
        )))
    }
}

#[async_trait]
impl<FS> ProviderNode for KubernetesNode<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn args(&self) -> Vec<&str> {
        self.args.iter().map(|arg| arg.as_str()).collect()
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    fn relay_data_dir(&self) -> &PathBuf {
        &self.relay_data_dir
    }

    fn scripts_dir(&self) -> &PathBuf {
        &self.scripts_dir
    }

    fn log_path(&self) -> &PathBuf {
        &self.log_path
    }

    fn path_in_node(&self, file: &Path) -> PathBuf {
        // here is just a noop op since we will receive the path
        // for the file inside the pod
        PathBuf::from(file)
    }

    async fn logs(&self) -> Result<String, ProviderError> {
        self.k8s_client
            .pod_logs(&self.namespace_name(), &self.name)
            .await
            .map_err(|err| ProviderError::GetLogsFailed(self.name.to_string(), err.into()))
    }

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError> {
        let logs = self.logs().await?;

        self.filesystem
            .write(local_dest, logs)
            .await
            .map_err(|err| ProviderError::DumpLogsFailed(self.name.to_string(), err.into()))?;

        Ok(())
    }

    async fn run_command(
        &self,
        options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let mut command = vec![];

        for (name, value) in options.env {
            command.push(format!("export {name}={value};"));
        }

        command.push(options.program);

        for arg in options.args {
            command.push(arg);
        }

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["sh", "-c", &command.join(" ")],
            )
            .await
            .map_err(|err| ProviderError::RunCommandError(self.name.to_string(), err.into()))
    }

    async fn run_script(
        &self,
        options: RunScriptOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        let file_name = options
            .local_script_path
            .file_name()
            .expect("file name should be present at this point")
            .to_string_lossy();

        self.run_command(RunCommandOptions {
            program: format!("/tmp/{file_name}"),
            args: options.args,
            env: options.env,
        })
        .await
        .map_err(|err| ProviderError::RunScriptError(self.name.to_string(), err.into()))
    }

    async fn send_file(
        &self,
        local_file_path: &Path,
        remote_file_path: &Path,
        mode: &str,
    ) -> Result<(), ProviderError> {
        let data = self.filesystem.read(local_file_path).await.unwrap();

        if let Some(remote_parent_dir) = self.get_remote_parent_dir(remote_file_path) {
            self.create_remote_dir(&remote_parent_dir).await?;
        }

        self.http_client
            .post(format!(
                "http://{}{}",
                self.file_server_local_host().await?,
                remote_file_path.to_string_lossy()
            ))
            .body(data)
            .send()
            .await
            .map_err(|err| {
                ProviderError::SendFile(local_file_path.to_string_lossy().to_string(), err.into())
            })?;

        let _ = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec![
                    "/cfg/curl",
                    &format!("fileserver{}", remote_file_path.to_string_lossy()),
                    "--output",
                    &remote_file_path.to_string_lossy(),
                ],
            )
            .await
            .map_err(|err| {
                ProviderError::SendFile(local_file_path.to_string_lossy().to_string(), err.into())
            })?;

        let _ = self
            .k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["chmod", mode, &remote_file_path.to_string_lossy()],
            )
            .await
            .map_err(|err| {
                ProviderError::SendFile(local_file_path.to_string_lossy().to_string(), err.into())
            })?;

        Ok(())
    }

    async fn receive_file(
        &self,
        _remote_src: &Path,
        _local_dest: &Path,
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["echo", "pause", ">", "/tmp/zombiepipe"],
            )
            .await
            .map_err(|err| ProviderError::PauseNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::PauseNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when pausing node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["echo", "resume", ">", "/tmp/zombiepipe"],
            )
            .await
            .map_err(|err| ProviderError::ResumeNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::ResumeNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when pausing node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError> {
        if let Some(duration) = after {
            sleep(duration).await;
        }

        self.k8s_client
            .pod_exec(
                &self.namespace_name(),
                &self.name,
                vec!["echo", "restart", ">", "/tmp/zombiepipe"],
            )
            .await
            .map_err(|err| ProviderError::RestartNodeFailed(self.name.to_string(), err.into()))?
            .map_err(|err| {
                ProviderError::RestartNodeFailed(
                    self.name.to_string(),
                    anyhow!("error when restarting node: status {}: {}", err.0, err.1),
                )
            })?;

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        self.k8s_client
            .delete_pod(&self.namespace_name(), &self.name)
            .await
            .map_err(|err| ProviderError::KillNodeFailed(self.name.to_string(), err.into()))?;

        if let Some(namespace) = self.namespace.upgrade() {
            namespace.nodes.write().await.remove(&self.name);
        }

        Ok(())
    }
}
