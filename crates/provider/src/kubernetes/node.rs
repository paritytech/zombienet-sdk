use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::anyhow;
use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::time::sleep;

use super::{client::KubernetesClient, namespace::WeakKubernetesNamespace};
use crate::{
    types::{ExecutionResult, Port, RunCommandOptions, RunScriptOptions},
    ProviderError, ProviderNode,
};

#[derive(Clone)]
pub(super) struct KubernetesNode<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) name: String,
    // TODO: find an easy way to avoid this given we have Weak to inner namespace but namespace name is one level up
    pub(super) namespace_name: String,
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) base_dir: PathBuf,
    pub(super) config_dir: PathBuf,
    pub(super) data_dir: PathBuf,
    pub(super) scripts_dir: PathBuf,
    pub(super) log_path: PathBuf,
    pub(super) filesystem: FS,
    pub(super) client: KC,
    pub(super) namespace: WeakKubernetesNamespace<FS, KC>,
}

#[async_trait]
impl<FS, KC> ProviderNode for KubernetesNode<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    KC: KubernetesClient<FS> + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn program(&self) -> &str {
        &self.program
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

    async fn ip(&self) -> Result<IpAddr, ProviderError> {
        todo!()
    }

    async fn endpoint(&self) -> Result<(IpAddr, Port), ProviderError> {
        todo!();
    }

    async fn mapped_port(&self, _port: Port) -> Result<Port, ProviderError> {
        todo!()
    }

    async fn logs(&self) -> Result<String, ProviderError> {
        self.client
            .pod_logs(&self.namespace_name, &self.name)
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

        self.client
            .pod_exec(
                &self.namespace_name,
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
        self.client
            .copy_to_pod(
                &self.namespace_name,
                &self.name,
                options.local_script_path.clone(),
                "/tmp".into(),
                "0755",
            )
            .await
            .map_err(|err| ProviderError::RunScriptError(self.name.to_string(), err.into()))?;

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

    async fn copy_file_from_node(
        &self,
        remote_src: PathBuf,
        local_dest: PathBuf,
    ) -> Result<(), ProviderError> {
        self.client
            .copy_from_pod(&self.namespace_name, &self.name, remote_src, local_dest)
            .await
            .map_err(|err| {
                ProviderError::CopyFileFromNodeError(self.name.to_string(), err.into())
            })?;

        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        self.client
            .pod_exec(
                &self.namespace_name,
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
        self.client
            .pod_exec(
                &self.namespace_name,
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

        self.client
            .pod_exec(
                &self.namespace_name,
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
        self.client
            .delete_pod(&self.namespace_name, &self.name)
            .await
            .map_err(|err| ProviderError::KillNodeFailed(self.name.to_string(), err.into()))?;

        if let Some(namespace) = self.namespace.inner.upgrade() {
            namespace.write().await.nodes.remove(&self.name);
        }

        Ok(())
    }
}
