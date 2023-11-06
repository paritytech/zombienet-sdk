mod fake;
mod kube_rs;

use std::{collections::BTreeMap, path::Path};

use async_trait::async_trait;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, PodSpec};
use support::fs::FileSystem;
use tokio::io::AsyncRead;

use crate::types::ExecutionResult;

#[async_trait]
pub trait KubernetesClient<FS>
where
    FS: FileSystem + Send + Sync,
{
    async fn get_namespace(&self, name: &str) -> kube::Result<Option<Namespace>>;

    async fn get_namespaces(&self) -> kube::Result<Vec<Namespace>>;

    async fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> kube::Result<Namespace>;

    async fn create_config_map_from_file(
        &self,
        namespace: &str,
        name: &str,
        file_name: &str,
        file_contents: &str,
        labels: BTreeMap<String, String>,
    ) -> kube::Result<ConfigMap>;

    async fn create_pod(
        &self,
        namespace: &str,
        name: &str,
        spec: PodSpec,
        labels: BTreeMap<String, String>,
    ) -> kube::Result<Pod>;

    async fn create_pod_logs_stream(
        &self,
        namespace: &str,
        name: &str,
    ) -> kube::Result<Box<dyn AsyncRead + Send + Unpin>>;

    async fn pod_exec<S>(
        &self,
        namespace: &str,
        name: &str,
        command: Vec<S>,
    ) -> kube::Result<ExecutionResult>
    where
        S: Into<String> + std::fmt::Debug + Send;

    async fn copy_to_pod<P>(
        &self,
        namespace: &str,
        name: &str,
        from: P,
        to: P,
    ) -> kube::Result<()>
    where
        P: AsRef<Path> + Send;
}
