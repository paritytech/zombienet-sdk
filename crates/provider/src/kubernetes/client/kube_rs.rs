use std::{
    collections::BTreeMap, fmt::Debug, os::unix::process::ExitStatusExt, path::Path,
    process::ExitStatus,
};

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, PodSpec};
use kube::{
    api::{AttachParams, ListParams, LogParams, PostParams, WatchParams},
    core::{ObjectMeta, WatchEvent},
    Api, Client, Resource,
};
use serde::de::DeserializeOwned;
use sha2::digest::Digest;
use support::fs::FileSystem;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::types::ExecutionResult;

use super::KubernetesClient;

pub struct KubeRsKubernetesClient<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    client: kube::Client,
    filesystem: FS,
}

impl<FS> KubeRsKubernetesClient<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    async fn new(filesystem: FS) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            // TODO: make it more flexible with path to kube config
            client: Client::try_default().await?,
            filesystem,
        })
    }

    async fn wait_created<K>(&self, api: Api<K>, name: &str) -> kube::Result<()>
    where
        K: Clone + DeserializeOwned + Debug,
    {
        let params = &WatchParams::default().fields(&format!("metadata.name={}", name));
        let mut stream = api.watch(&params, "0").await.unwrap().boxed();

        while let Some(status) = stream.try_next().await.unwrap() {
            match status {
                WatchEvent::Added(_) => break,
                WatchEvent::Error(err) => Err(kube::Error::Api(err))?,
                _ => panic!("Unexpected event happened while creating '{}'", name),
            }
        }

        Ok(())
    }
}

#[async_trait]
impl<FS> KubernetesClient<FS> for KubeRsKubernetesClient<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    async fn get_namespace(&self, name: &str) -> kube::Result<Option<Namespace>> {
        Api::<Namespace>::all(self.client.clone())
            .get_opt(name.as_ref())
            .await
    }

    async fn get_namespaces(&self) -> kube::Result<Vec<Namespace>> {
        Ok(Api::<Namespace>::all(self.client.clone())
            .list(&ListParams::default())
            .await?
            .into_iter()
            .filter(|ns| matches!(&ns.meta().name, Some(name) if name.starts_with("zombienet")))
            .collect())
    }

    async fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> kube::Result<Namespace> {
        let namespaces = Api::<Namespace>::all(self.client.clone());
        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        };

        namespaces
            .create(&PostParams::default(), &namespace)
            .await?;

        self.wait_created(namespaces, name).await?;

        Ok(namespace)
    }

    async fn create_config_map_from_file(
        &self,
        namespace: &str,
        name: &str,
        file_name: &str,
        file_contents: &str,
        labels: BTreeMap<String, String>,
    ) -> kube::Result<ConfigMap> {
        let config_maps = Api::<ConfigMap>::namespaced(self.client.clone(), namespace);
        let config_map = ConfigMap {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            data: Some(BTreeMap::from([(
                file_name.to_string(),
                file_contents.to_string(),
            )])),
            ..Default::default()
        };

        config_maps
            .create(&PostParams::default(), &config_map)
            .await?;

        self.wait_created(config_maps, name).await?;

        Ok(config_map)
    }

    async fn create_pod(
        &self,
        namespace: &str,
        name: &str,
        spec: PodSpec,
        labels: BTreeMap<String, String>,
    ) -> kube::Result<Pod> {
        let pods = Api::<Pod>::namespaced(self.client.clone(), namespace);
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(spec),
            ..Default::default()
        };

        pods.create(&PostParams::default(), &pod).await?;

        self.wait_created(pods, name).await?;

        Ok(pod)
    }

    async fn create_pod_logs_stream(
        &self,
        namespace: &str,
        name: &str,
    ) -> kube::Result<Box<dyn AsyncRead + Send + Unpin>> {
        Ok(Box::new(
            Api::<Pod>::namespaced(self.client.clone(), namespace)
                .log_stream(
                    name,
                    &LogParams {
                        follow: true,
                        pretty: true,
                        timestamps: true,
                        ..Default::default()
                    },
                )
                .await?
                .compat(),
        ))
    }

    async fn pod_exec<S>(
        &self,
        namespace: &str,
        name: &str,
        command: Vec<S>,
    ) -> kube::Result<ExecutionResult>
    where
        S: Into<String> + std::fmt::Debug + Send,
    {
        let mut process = Api::<Pod>::namespaced(self.client.clone(), namespace)
            .exec(
                name,
                command,
                &AttachParams::default().stdout(true).stderr(true),
            )
            .await?;

        // retrieve stdout/stderr and k8s execution status
        let mut stdout_stream = process
            .stdout()
            .take()
            .expect("exec with stdout set to true shouldn't fail");
        let mut stderr_stream = process
            .stderr()
            .take()
            .expect("exec with stderr set to true shouldn't fail");
        let status = process
            .take_status()
            .expect("first call to status shouldn't fail")
            .await;

        // await process to finish
        process.join().await.unwrap();

        match status {
            // command succeeded with stdout
            Some(status) if status.status.as_ref().is_some_and(|s| s == "Success") => {
                let mut stdout = String::new();
                stdout_stream.read_to_string(&mut stdout).await.unwrap();
                Ok(Ok(stdout))
            },
            // command failed
            Some(status) if status.status.as_ref().is_some_and(|s| s == "Failure") => {
                match status.reason {
                    // due to exit code
                    Some(reason) if reason == "NonZeroExitCode" => {
                        let exit_status = status
                            .details
                            .and_then(|details| {
                                details.causes.and_then(|causes| {
                                    causes.first().and_then(|cause| {
                                        cause.message.as_deref().and_then(|message| {
                                            message
                                                .parse::<i32>()
                                                .ok()
                                                .map(|code| ExitStatus::from_raw(code))
                                        })
                                    })
                                })
                            })
                            .expect(
                                "command with non-zero exit code should have exit code present",
                            );

                        let mut stderr = String::new();
                        stderr_stream.read_to_string(&mut stderr).await.unwrap();

                        Ok(Err((exit_status, stderr)))
                    },
                    // due to other reason (e.g.: binary not found)
                    // TODO: build error correctly, using anyhow to simplify?
                    Some(_reason) => todo!(),
                    None => {
                        panic!("command had status but no reason was present, this is a bug");
                    },
                }
            },
            Some(_) => {
                unreachable!("command had status but it didn't matches either Success or Failure, this is a bug from the kubers library itself");
            },
            None => {
                panic!("command has no status following its execution, this is a bug");
            },
        }
    }

    // TODO: rework error to have more explicit message instead of just passing the underlying error?
    async fn copy_to_pod<P>(
        &self,
        namespace: &str,
        name: &str,
        from: P,
        to: P,
        mode: &str,
    ) -> kube::Result<()>
    where
        P: AsRef<Path> + Send,
    {
        let pods = Api::<Pod>::namespaced(self.client.clone(), namespace);
        let file_name = from.as_ref().file_name().unwrap().to_owned();
        let contents = self.filesystem.read(from).await.unwrap();

        // create archive header
        let mut header = tar::Header::new_gnu();
        header
            .set_path(&file_name)
            .map_err(|err| kube::Error::Service(err.into()))?;
        header.set_size(contents.len() as u64);
        header.set_cksum();

        // build archive from file contents
        let mut archive = tar::Builder::new(Vec::new());
        archive
            .append(&header, &mut contents.as_slice())
            .map_err(|err| kube::Error::Service(err.into()))?;
        let data = archive
            .into_inner()
            .map_err(|err| kube::Error::Service(err.into()))?;

        // execute tar command
        let dest = to.as_ref().to_string_lossy();
        let mut tar_process = pods
            .exec(
                name,
                vec!["tar", "-xmf", "-", "-C", &dest],
                &AttachParams::default().stdin(true),
            )
            .await?;

        // write archive content to attached process
        tar_process
            .stdin()
            .unwrap()
            .write_all(&data)
            .await
            .map_err(|err| kube::Error::Service(err.into()))?;

        // wait for process to finish
        tar_process
            .join()
            .await
            .map_err(|err| kube::Error::Service(err.into()))?;

        let file_path = format!(
            "{}/{}",
            to.as_ref().to_string_lossy(),
            file_name.to_string_lossy()
        );

        // execute chmod to set default file permissions
        self.pod_exec(namespace, name, vec!["chmod", &mode, &file_path])
            .await?
            .map_err(|err| {
                kube::Error::Service(anyhow!("error: status {}: {}", err.0, err.1).into())
            })?;

        // retrieve sha256sum of file to ensure integrity
        let sha256sum_stdout = self
            .pod_exec(namespace, name, vec!["sha256sum", &file_path])
            .await?
            .map_err(|err| {
                kube::Error::Service(anyhow!("error: status {}: {}", err.0, err.1).into())
            })?;
        let actual_hash = sha256sum_stdout
            .split_whitespace()
            .next()
            .expect("sha256sum output should be in the form `hash<spaces>filename`");

        // get the hash of the file
        let expected_hash = hex::encode(sha2::Sha256::digest(&contents));
        if actual_hash != expected_hash {
            // TODO: should we delete partially copied file here?
            return Err(kube::Error::Service(
                anyhow!(
                    "file copy failed, expected sha256sum of {} got {}",
                    expected_hash,
                    actual_hash
                )
                .into(),
            ));
        }

        Ok(())
    }
}
