mod kube_rs;

use std::{collections::BTreeMap, path::Path};

use async_trait::async_trait;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, PodSpec};
use support::fs::FileSystem;
use tokio::io::AsyncRead;

use crate::types::ExecutionResult;

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct Error(#[from] anyhow::Error);

pub type Result<T> = core::result::Result<T, Error>;

#[async_trait]
pub trait KubernetesClient<FS>
where
    FS: FileSystem + Send + Sync,
{
    async fn get_namespace(&self, name: &str) -> Result<Option<Namespace>>;

    async fn get_namespaces(&self) -> Result<Vec<Namespace>>;

    async fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<Namespace>;

    async fn create_config_map_from_file(
        &self,
        namespace: &str,
        name: &str,
        file_name: &str,
        file_contents: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<ConfigMap>;

    async fn create_pod(
        &self,
        namespace: &str,
        name: &str,
        spec: PodSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<Pod>;

    async fn pod_logs(&self, namespace: &str, name: &str) -> Result<String>;

    async fn create_pod_logs_stream(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>>;

    async fn pod_exec<S>(
        &self,
        namespace: &str,
        name: &str,
        command: Vec<S>,
    ) -> Result<ExecutionResult>
    where
        S: Into<String> + std::fmt::Debug + Send;

    async fn copy_to_pod<P>(
        &self,
        namespace: &str,
        name: &str,
        from: P,
        to: P,
        mode: &str,
    ) -> Result<()>
    where
        P: AsRef<Path> + Send;

    async fn copy_from_pod<P>(&self, namespace: &str, name: &str, from: P, to: P) -> Result<()>
    where
        P: AsRef<Path> + Send;

    async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()>;
}

pub use kube_rs::KubeRsKubernetesClient;