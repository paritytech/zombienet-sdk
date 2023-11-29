use std::{net::IpAddr, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::{sync::RwLock, task::JoinHandle, time::sleep};

use crate::{
    types::{ExecutionResult, Port, RunCommandOptions, RunScriptOptions},
    ProviderError, ProviderNode,
};

use super::{client::KubernetesClient, namespace::WeakKubernetesNamespace};

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
    pub(super) env: Vec<(String, String)>,
    pub(super) base_dir: PathBuf,
    pub(super) config_dir: PathBuf,
    pub(super) data_dir: PathBuf,
    pub(super) scripts_dir: PathBuf,
    pub(super) log_path: PathBuf,
    pub(super) inner: Arc<RwLock<KubernetesNodeInner>>,
    pub(super) filesystem: FS,
    pub(super) client: KC,
    pub(super) namespace: WeakKubernetesNamespace<FS, KC>,
}

pub(super) struct KubernetesNodeInner {
    pub(super) log_reading_handle: JoinHandle<()>,
    pub(super) log_writing_handle: JoinHandle<()>,
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
        todo!()
    }

    async fn dump_logs(&self, local_dest: PathBuf) -> Result<(), ProviderError> {
        todo!()
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

        Ok(self
            .client
            .pod_exec(
                &self.namespace_name,
                &self.name,
                vec!["sh", "-c", &command.join(" ")],
            )
            .await
            .unwrap())
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
            .unwrap();

        let file_name = options
            .local_script_path
            .file_name()
            .expect("file name should be present at this point")
            .to_string_lossy();

        Ok(self
            .run_command(RunCommandOptions {
                program: format!("/tmp/{file_name}"),
                args: options.args,
                env: options.env,
            })
            .await
            .unwrap())
    }

    async fn copy_file_from_node(
        &self,
        remote_src: PathBuf,
        local_dest: PathBuf,
    ) -> Result<(), ProviderError> {
        self.client
            .copy_from_pod(&self.namespace_name, &self.name, remote_src, local_dest)
            .await
            .unwrap();

        Ok(())
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        // TODO: handle unwraps
        self.client
            .pod_exec(
                &self.namespace_name,
                &self.name,
                vec!["echo", "pause", ">", "/tmp/zombiepipe"],
            )
            .await
            .unwrap()
            .unwrap();

        Ok(())
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        // TODO: handle unwraps
        self.client
            .pod_exec(
                &self.namespace_name,
                &self.name,
                vec!["echo", "resume", ">", "/tmp/zombiepipe"],
            )
            .await
            .unwrap()
            .unwrap();

        Ok(())
    }

    async fn restart(&self, after: Option<Duration>) -> Result<(), ProviderError> {
        if let Some(duration) = after {
            sleep(duration).await;
        }

        // TODO: handle unwraps
        self.client
            .pod_exec(
                &self.namespace_name,
                &self.name,
                vec!["echo", "restart", ">", "/tmp/zombiepipe"],
            )
            .await
            .unwrap()
            .unwrap();

        Ok(())
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        let inner = self.inner.write().await;

        inner.log_writing_handle.abort();
        inner.log_reading_handle.abort();
        self.client
            .delete_pod(&self.namespace_name, &self.name)
            .await
            .unwrap();

        if let Some(namespace) = self.namespace.inner.upgrade() {
            namespace.write().await.nodes.remove(&self.name);
        }

        Ok(())
    }
}
