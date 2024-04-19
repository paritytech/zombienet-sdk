use std::{
    collections::BTreeMap, fmt::Debug, os::unix::process::ExitStatusExt, process::ExitStatus,
    time::Duration,
};

use anyhow::anyhow;
use configuration::shared::constants::THIS_IS_A_BUG;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{
    ConfigMap, Namespace, Pod, PodSpec, PodStatus, Service, ServiceSpec,
};
use kube::{
    api::{AttachParams, DeleteParams, ListParams, LogParams, PostParams, WatchParams},
    core::{DynamicObject, GroupVersionKind, ObjectMeta, TypeMeta, WatchEvent},
    discovery::ApiResource,
    runtime::{conditions, wait::await_condition},
    Api, Resource,
};
use serde::de::DeserializeOwned;
use tokio::{io::AsyncRead, net::TcpListener, task::JoinHandle};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, trace};

use crate::{constants::LOCALHOST, types::ExecutionResult};

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct Error(#[from] anyhow::Error);

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone)]
pub struct KubernetesClient {
    inner: kube::Client,
}

impl KubernetesClient {
    pub(super) async fn new() -> Result<Self> {
        Ok(Self {
            // TODO: make it more flexible with path to kube config
            inner: kube::Client::try_default()
                .await
                .map_err(|err| Error::from(anyhow!("error initializing kubers client: {err}")))?,
        })
    }

    #[allow(dead_code)]
    pub(super) async fn get_namespace(&self, name: &str) -> Result<Option<Namespace>> {
        Api::<Namespace>::all(self.inner.clone())
            .get_opt(name.as_ref())
            .await
            .map_err(|err| Error::from(anyhow!("error while getting namespace {name}: {err}")))
    }

    #[allow(dead_code)]
    pub(super) async fn get_namespaces(&self) -> Result<Vec<Namespace>> {
        Ok(Api::<Namespace>::all(self.inner.clone())
            .list(&ListParams::default())
            .await
            .map_err(|err| Error::from(anyhow!("error while getting all namespaces: {err}")))?
            .into_iter()
            .filter(|ns| matches!(&ns.meta().name, Some(name) if name.starts_with("zombienet")))
            .collect())
    }

    pub(super) async fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<Namespace> {
        let namespaces = Api::<Namespace>::all(self.inner.clone());

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
            .await
            .map_err(|err| Error::from(anyhow!("error while created namespace {name}: {err}")))?;

        self.wait_created(namespaces, name).await?;

        Ok(namespace)
    }

    pub(super) async fn delete_namespace(&self, name: &str) -> Result<()> {
        let namespaces = Api::<Namespace>::all(self.inner.clone());

        namespaces
            .delete(name, &DeleteParams::default())
            .await
            .map_err(|err| Error::from(anyhow!("error while deleting namespace {name}: {err}")))?;

        Ok(())
    }

    pub(super) async fn create_config_map_from_file(
        &self,
        namespace: &str,
        name: &str,
        file_name: &str,
        file_contents: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<ConfigMap> {
        let config_maps = Api::<ConfigMap>::namespaced(self.inner.clone(), namespace);

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
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error while creating config map {name} for {file_name}: {err}"
                ))
            })?;

        self.wait_created(config_maps, name).await?;

        Ok(config_map)
    }

    pub(super) async fn create_pod(
        &self,
        namespace: &str,
        name: &str,
        spec: PodSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<Pod> {
        let pods = Api::<Pod>::namespaced(self.inner.clone(), namespace);

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

        pods.create(&PostParams::default(), &pod)
            .await
            .map_err(|err| Error::from(anyhow!("error while creating pod {name}: {err}")))?;

        trace!("Pod {name} checking for ready state!");
        let wait_ready = await_condition(pods, name, helpers::is_pod_ready());
        // TODO: we may want to allow to set this timeout for pod spawning.
        let _ = tokio::time::timeout(Duration::from_secs(30), wait_ready)
            .await
            .map_err(|err| {
                Error::from(anyhow!("error while awaiting pod {name} running: {err}"))
            })?;

        debug!("Pod {name} is Ready!");
        Ok(pod)
    }

    pub(super) async fn pod_logs(&self, namespace: &str, name: &str) -> Result<String> {
        Api::<Pod>::namespaced(self.inner.clone(), namespace)
            .logs(
                name,
                &LogParams {
                    pretty: true,
                    timestamps: true,
                    ..Default::default()
                },
            )
            .await
            .map_err(|err| Error::from(anyhow!("error while getting logs for pod {name}: {err}")))
    }

    pub(super) async fn pod_status(&self, namespace: &str, name: &str) -> Result<PodStatus> {
        let pod = Api::<Pod>::namespaced(self.inner.clone(), namespace)
            .get(name)
            .await
            .map_err(|err| Error::from(anyhow!("error while getting pod {name}: {err}")))?;

        let status = pod.status.ok_or(Error::from(anyhow!(
            "error while getting status for pod {name}"
        )))?;
        Ok(status)
    }

    #[allow(dead_code)]
    pub(super) async fn create_pod_logs_stream(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        Ok(Box::new(
            Api::<Pod>::namespaced(self.inner.clone(), namespace)
                .log_stream(
                    name,
                    &LogParams {
                        follow: true,
                        pretty: true,
                        timestamps: true,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|err| {
                    Error::from(anyhow!(
                        "error while getting a log stream for {name}: {err}"
                    ))
                })?
                .compat(),
        ))
    }

    pub(super) async fn pod_exec<S>(
        &self,
        namespace: &str,
        name: &str,
        command: Vec<S>,
    ) -> Result<ExecutionResult>
    where
        S: Into<String> + std::fmt::Debug + Send,
    {
        let mut process = Api::<Pod>::namespaced(self.inner.clone(), namespace)
            .exec(
                name,
                command,
                &AttachParams::default().stdout(true).stderr(true),
            )
            .await
            .map_err(|err| Error::from(anyhow!("error while exec in the pod {name}: {err}")))?;

        let stdout_stream = process.stdout().expect(&format!(
            "stdout shouldn't be None when true passed to exec {THIS_IS_A_BUG}"
        ));
        let stdout = tokio_util::io::ReaderStream::new(stdout_stream)
            .filter_map(|r| async { r.ok().and_then(|v| String::from_utf8(v.to_vec()).ok()) })
            .collect::<Vec<_>>()
            .await
            .join("");
        let stderr_stream = process.stderr().expect(&format!(
            "stderr shouldn't be None when true passed to exec {THIS_IS_A_BUG}"
        ));
        let stderr = tokio_util::io::ReaderStream::new(stderr_stream)
            .filter_map(|r| async { r.ok().and_then(|v| String::from_utf8(v.to_vec()).ok()) })
            .collect::<Vec<_>>()
            .await
            .join("");

        let status = process
            .take_status()
            .expect(&format!(
                "first call to status shouldn't fail {THIS_IS_A_BUG}"
            ))
            .await;

        // await process to finish
        process.join().await.map_err(|err| {
            Error::from(anyhow!(
                "error while joining process during exec for {name}: {err}"
            ))
        })?;

        match status {
            // command succeeded with stdout
            Some(status) if status.status.as_ref().is_some_and(|s| s == "Success") => {
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
                                            message.parse::<i32>().ok().map(ExitStatus::from_raw)
                                        })
                                    })
                                })
                            })
                            .expect(
                                &format!("command with non-zero exit code should have exit code present {THIS_IS_A_BUG}")
                            );

                        Ok(Err((exit_status, stderr)))
                    },
                    // due to other unknown reason
                    Some(ref reason) => Err(Error::from(anyhow!(
                        format!("unhandled reason while exec for {name}: {reason}, stderr: {stderr}, status: {:?}", status)
                    ))),
                    None => {
                        panic!("command had status but no reason was present, this is a bug")
                    },
                }
            },
            Some(_) => {
                unreachable!("command had status but it didn't matches either Success or Failure, this is a bug from the kube.rs library itself");
            },
            None => {
                panic!("command has no status following its execution, this is a bug");
            },
        }
    }

    pub(super) async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()> {
        let pods = Api::<Pod>::namespaced(self.inner.clone(), namespace);

        pods.delete(name, &DeleteParams::default())
            .await
            .map_err(|err| Error::from(anyhow!("error when deleting pod {name}: {err}")))?;

        await_condition(pods, name, conditions::is_deleted(name))
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error when waiting for pod {name} to be deleted: {err}"
                ))
            })?;

        Ok(())
    }

    pub(super) async fn create_service(
        &self,
        namespace: &str,
        name: &str,
        spec: ServiceSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<Service> {
        let services = Api::<Service>::namespaced(self.inner.clone(), namespace);

        let service = Service {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            spec: Some(spec),
            ..Default::default()
        };

        services
            .create(&PostParams::default(), &service)
            .await
            .map_err(|err| Error::from(anyhow!("error while creating service {name}: {err}")))?;

        Ok(service)
    }

    // TODO: remove `unwrap` and add logic to handle panic in spawned task.
    // We should try to recreate the port-fw at least a couple of times before give up.
    pub(super) async fn create_pod_port_forward(
        &self,
        namespace: &str,
        name: &str,
        local_port: u16,
        remote_port: u16,
    ) -> Result<(u16, JoinHandle<()>)> {
        let pods = Api::<Pod>::namespaced(self.inner.clone(), namespace);
        let bind = TcpListener::bind((LOCALHOST, local_port))
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error binding port {local_port} for  pod {name}: {err}"
                ))
            })?;
        let local_port = bind.local_addr().map_err(|err| Error(err.into()))?.port();
        let name = name.to_string();

        Ok((
            local_port,
            tokio::spawn(async move {
                loop {
                    let (mut client_conn, _) = bind.accept().await.unwrap();
                    let (name, pods) = (name.clone(), pods.clone());

                    tokio::spawn(async move {
                        let mut forwarder = pods.portforward(&name, &[remote_port]).await.unwrap();
                        let mut upstream_conn = forwarder.take_stream(remote_port).unwrap();

                        tokio::io::copy_bidirectional(&mut client_conn, &mut upstream_conn)
                            .await
                            .unwrap();

                        drop(upstream_conn);

                        forwarder.join().await.unwrap();
                    });
                }
            }),
        ))
    }

    /// Create resources from yamls in `static-configs` directory
    pub(super) async fn create_static_resource(
        &self,
        namespace: &str,
        raw_manifest: &str,
    ) -> Result<()> {
        let tm: TypeMeta = serde_yaml::from_str(raw_manifest).map_err(|err| {
            Error::from(anyhow!(
                "error while extracting TypeMeta from manifest: {raw_manifest}: {err}"
            ))
        })?;
        let gvk = GroupVersionKind::try_from(&tm).map_err(|err| {
            Error::from(anyhow!(
                "error while extracting GroupVersionKind from manifest: {raw_manifest}: {err}"
            ))
        })?;

        let ar = ApiResource::from_gvk(&gvk);
        let api: Api<DynamicObject> = Api::namespaced_with(self.inner.clone(), namespace, &ar);

        api.create(
            &PostParams::default(),
            &serde_yaml::from_str(raw_manifest).unwrap(),
        )
        .await
        .map_err(|err| {
            Error::from(anyhow!(
                "error while creating static-config {raw_manifest}: {err}"
            ))
        })?;

        Ok(())
    }

    async fn wait_created<K>(&self, api: Api<K>, name: &str) -> Result<()>
    where
        K: Clone + DeserializeOwned + Debug,
    {
        let params = &WatchParams::default().fields(&format!("metadata.name={}", name));
        let mut stream = api
            .watch(params, "0")
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error while awaiting first response when resource {name} is created: {err}"
                ))
            })?
            .boxed();

        while let Some(status) = stream.try_next().await.map_err(|err| {
            Error::from(anyhow!(
                "error while awaiting next change after resource {name} is created: {err}"
            ))
        })? {
            match status {
                WatchEvent::Added(_) => break,
                WatchEvent::Error(err) => Err(Error::from(anyhow!(
                    "error while awaiting resource {name} is created: {err}"
                )))?,
                _ => panic!(
                    "Unexpected event happened while creating '{}' {THIS_IS_A_BUG}",
                    name
                ),
            }
        }

        Ok(())
    }
}

mod helpers {
    use k8s_openapi::api::core::v1::Pod;
    use kube::runtime::wait::Condition;
    use tracing::trace;

    /// An await condition for `Pod` that returns `true` once it is ready
    /// based on [`kube::runtime::wait::conditions::is_pod_running`]
    pub fn is_pod_ready() -> impl Condition<Pod> {
        |obj: Option<&Pod>| {
            if let Some(pod) = &obj {
                if let Some(status) = &pod.status {
                    if let Some(conditions) = &status.conditions {
                        let ready = conditions
                            .iter()
                            .any(|cond| cond.status == "True" && cond.type_ == "Ready");

                        if ready {
                            trace!("{:#?}", status);
                            return ready;
                        }
                    }
                }
            }
            false
        }
    }
}
