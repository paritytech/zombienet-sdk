use std::{
    collections::BTreeMap,
    fmt::Debug,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{ExitStatus, Stdio},
};

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, Pod, PodSpec, Service, ServiceSpec};
use kube::{
    api::{AttachParams, DeleteParams, ListParams, LogParams, PostParams, WatchParams},
    core::{ObjectMeta, WatchEvent},
    runtime::{conditions, wait::await_condition},
    Api, Client, Resource,
};
use serde::de::DeserializeOwned;
use sha2::{digest::Digest, Sha256};
use support::fs::FileSystem;
use tokio::{
    fs::File,
    io::{self, AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use super::{Error, KubernetesClient, Result};
use crate::types::ExecutionResult;

#[derive(Clone)]
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
    pub async fn new(filesystem: FS) -> Result<Self> {
        Ok(Self {
            // TODO: make it more flexible with path to kube config
            client: Client::try_default()
                .await
                .map_err(|err| Error::from(anyhow!("error initializing kubers client: {err}")))?,
            filesystem,
        })
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
    async fn get_namespace(&self, name: &str) -> Result<Option<Namespace>> {
        Api::<Namespace>::all(self.client.clone())
            .get_opt(name.as_ref())
            .await
            .map_err(|err| Error::from(anyhow!("error while getting namespace {name}: {err}")))
    }

    async fn get_namespaces(&self) -> Result<Vec<Namespace>> {
        Ok(Api::<Namespace>::all(self.client.clone())
            .list(&ListParams::default())
            .await
            .map_err(|err| Error::from(anyhow!("error while getting all namespaces: {err}")))?
            .into_iter()
            .filter(|ns| matches!(&ns.meta().name, Some(name) if name.starts_with("zombienet")))
            .collect())
    }

    async fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<Namespace> {
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
            .await
            .map_err(|err| Error::from(anyhow!("error while created namespace {name}: {err}")))?;

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
    ) -> Result<ConfigMap> {
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
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error while creating config map {name} for {file_name}: {err}"
                ))
            })?;

        self.wait_created(config_maps, name).await?;

        Ok(config_map)
    }

    async fn create_pod(
        &self,
        namespace: &str,
        name: &str,
        spec: PodSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<Pod> {
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

        pods.create(&PostParams::default(), &pod)
            .await
            .map_err(|err| Error::from(anyhow!("error while creating pod {name}: {err}")))?;

        await_condition(pods, name, conditions::is_pod_running())
            .await
            .map_err(|err| {
                Error::from(anyhow!("error while awaiting pod {name} running: {err}"))
            })?;

        Ok(pod)
    }

    async fn pod_logs(&self, namespace: &str, name: &str) -> Result<String> {
        Api::<Pod>::namespaced(self.client.clone(), namespace)
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

    async fn create_pod_logs_stream(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
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
                .await
                .map_err(|err| {
                    Error::from(anyhow!(
                        "error while getting a log stream for {name}: {err}"
                    ))
                })?
                .compat(),
        ))
    }

    async fn pod_exec<S>(
        &self,
        namespace: &str,
        name: &str,
        command: Vec<S>,
    ) -> Result<ExecutionResult>
    where
        S: Into<String> + std::fmt::Debug + Send,
    {
        let mut process = Api::<Pod>::namespaced(self.client.clone(), namespace)
            .exec(
                name,
                command,
                &AttachParams::default().stdout(true).stderr(true),
            )
            .await
            .map_err(|err| Error::from(anyhow!("error while exec in the pod {name}: {err}")))?;

        let stdout_stream = process
            .stdout()
            .expect("stdout shouldn't be None when true passed to exec");
        let stdout = tokio_util::io::ReaderStream::new(stdout_stream)
            .filter_map(|r| async { r.ok().and_then(|v| String::from_utf8(v.to_vec()).ok()) })
            .collect::<Vec<_>>()
            .await
            .join("");
        let stderr_stream = process
            .stderr()
            .expect("stderr shouldn't be None when true passed to exec");
        let stderr = tokio_util::io::ReaderStream::new(stderr_stream)
            .filter_map(|r| async { r.ok().and_then(|v| String::from_utf8(v.to_vec()).ok()) })
            .collect::<Vec<_>>()
            .await
            .join("");

        let status = process
            .take_status()
            .expect("first call to status shouldn't fail")
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
                                "command with non-zero exit code should have exit code present",
                            );

                        Ok(Err((exit_status, stderr)))
                    },
                    // due to other unknown reason
                    Some(reason) => Err(Error::from(anyhow!(
                        "unhandled reason while exec for {name}: {reason}"
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

    async fn copy_to_pod<P>(
        &self,
        namespace: &str,
        name: &str,
        from: P,
        to: P,
        mode: &str,
    ) -> Result<()>
    where
        P: AsRef<Path> + Send,
    {
        let pods = Api::<Pod>::namespaced(self.client.clone(), namespace);
        let file_name = to
            .as_ref()
            .file_name()
            .ok_or(Error::from(anyhow!(
                "error while copying to pod {name}: no filename was present in `to` path {}",
                to.as_ref().to_string_lossy()
            )))?
            .to_owned();
        let contents = self.filesystem.read(from).await.map_err(|err| {
            Error::from(anyhow!(
                "error while reading {} when trying to copy file to pod {name}: {err}",
                to.as_ref().to_string_lossy()
            ))
        })?;

        // create archive header
        let mut header = tar::Header::new_gnu();
        header.set_path(&file_name).map_err(|err| {
            Error::from(anyhow!(
                "error while setting path with {} for archive when trying to copy file {} to pod {name}: {err}",
                file_name.to_string_lossy(),
                to.as_ref().to_string_lossy()
            ))
        })?;
        header.set_size(contents.len() as u64);
        header.set_cksum();

        // build archive from file contents
        let mut archive = tar::Builder::new(Vec::new());
        archive
            .append(&header, &mut contents.as_slice())
            .map_err(|err| {
                Error::from(anyhow!(
                    "error while appending content of {} to archive when trying to copy file to pod {name}: {err}",
                    file_name.to_string_lossy(),
                ))
            })?;

        let data = archive.into_inner().map_err(|err| {
            Error::from(anyhow!(
                "error while unwraping archive when trying to copy file {} to pod {name}: {err}",
                file_name.to_string_lossy()
            ))
        })?;

        // execute tar command
        let dir_dest = to
            .as_ref()
            .parent()
            .ok_or(Error::from(anyhow!(
                "error while unwraping destination parent (to: {})",
                to.as_ref().to_string_lossy()
            )))?
            .to_string_lossy();

        let mut tar_process = pods
            .exec(
                name,
                vec!["tar", "-xmf", "-", "-C", &dir_dest],
                &AttachParams::default().stdin(true).stderr(false),
            )
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error while executing tar when trying to copy file {} to pod {name}: {err}",
                    file_name.to_string_lossy()
                ))
            })?;

        println!("executing write all");
        // write archive content to attached process
        tar_process
            .stdin()
            .expect("stdin shouldn't be None when true passed to exec")
            .write_all(&data)
            .await
            .map_err(|err| Error::from(anyhow!("error when writing file {} content as archive to tar process when trying to copy file to pod {name}: {err}",file_name.to_string_lossy())))?;

        // wait for process to finish
        tar_process
            .join()
            .await
            .map_err(|err| Error::from(anyhow!("error while trying to join the tar process when copying file {} to pod {name}: {err}", file_name.to_string_lossy())))?;

        // TODO: check this logic since `to` should be the path of the pod
        let file_path = to.as_ref().to_string_lossy().to_string();

        // execute chmod to set file permissions
        println!("executing chmod {} {}", &mode, &file_path);
        let _ = self.pod_exec(namespace, name, vec!["chmod", &mode, &file_path])
            .await
            .map_err(|err| Error::from(anyhow!("error while trying to setting permissions when trying to copy file {} to pod {name}: status {}: {}", file_name.to_string_lossy(), err.0, err.0)))?
            .map_err(|err| Error::from(anyhow!("error happened when chmoding file {} when trying to copy file to pod {name}: status {}: {}", file_name.to_string_lossy(), err.0, err.1)))?;

        // retrieve sha256sum of file to ensure integrity
        let sha256sum_stdout = self
            .pod_exec(namespace, name, vec!["sha256sum", &file_path])
            .await
            .map_err(|err| Error::from(anyhow!("error while exec for sha256 integrity check when trying to copy file {} to {name}: {err}", file_name.to_string_lossy())))?
            .map_err(|err| Error::from(anyhow!("sha256 integrity check failed when trying to copy file {} to {name}: status {}: {}", file_name.to_string_lossy(), err.0, err.1)))?;
        let actual_hash = sha256sum_stdout
            .split_whitespace()
            .next()
            .expect("sha256sum output should be in the form `hash<spaces>filename`");

        // get the hash of the file
        let expected_hash = hex::encode(sha2::Sha256::digest(&contents));
        if actual_hash != expected_hash {
            return Err(Error::from(anyhow!(
                "file {} copy to {name} failed, expected sha256sum of {} got {}",
                file_name.to_string_lossy(),
                expected_hash,
                actual_hash
            )));
        }

        Ok(())
    }

    async fn copy_from_pod<P>(&self, namespace: &str, name: &str, from: P, to: P) -> Result<()>
    where
        P: AsRef<Path> + Send,
    {
        let pods = Api::<Pod>::namespaced(self.client.clone(), namespace);
        let file_name = from
            .as_ref()
            .file_name()
            .ok_or(Error::from(anyhow!(
                "no file name found when trying to copy file {} from pod {}",
                from.as_ref().to_string_lossy(),
                name
            )))?
            .to_string_lossy();
        let file_dir = to
            .as_ref()
            .parent()
            .ok_or(Error::from(anyhow!(
                "no parent dir found for file {file_name} when trying to copy file from pod {name}"
            )))?
            .to_string_lossy();

        // create the archive in the pod and pipe to stdout
        let mut tar = pods
            .exec(
                name,
                vec!["tar", "-cf", "-", "-C", &file_dir, &file_name],
                &AttachParams::default().stdin(true),
            )
            .await
            .map_err(|err| {
                Error::from(anyhow!(
                    "error while executing tar command when trying to copy {file_name} from {name}: {err}",
                ))
            })?;

        let mut tar_stdout = tar
            .stdout()
            .take()
            .expect("stdout shouldn't be None when true passed to exec");

        // create child process tar fo extraction
        let dest_dir = to.as_ref().to_string_lossy();
        let mut extract = Command::new("tar")
            .args(vec!["-xmf", "-", "-C", &dest_dir, &file_name])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                Error::from(anyhow!(
                    "error when spawning tar process when copying {file_name} from pod {name}: {err}",
                ))
            })?;

        let mut extract_stdin = extract.stdin.take().ok_or(Error::from(anyhow!(
            "error when getting stdin from tar process when copying {file_name} from pod {name}"
        )))?;

        // pipe the container tar stdout into the local tar stdin
        io::copy(&mut tar_stdout, &mut extract_stdin).await.map_err(|err| Error::from(anyhow!("error when piping tar stdout to tar stdin when copying {file_name} from pod {name}: {err}",)))?;

        tar.join().await.map_err(|err| {
            Error::from(anyhow!(
                "error when joining tar process when copying {file_name} from pod {name}: {err}",
            ))
        })?;
        extract.wait().await.map_err(|err| {
            Error::from(anyhow!(
                "error when waiting for tar process when copying {file_name} from pod {name}: {err}",
            ))
        })?;

        // compute sha256sum of local received file
        let dest_path = format!("{dest_dir}/{file_name}");
        let mut file = File::open(&dest_path).await.map_err(|err| {
            Error::from(anyhow!(
                "error when opening file {file_name} when copying from pod {name}: {err}",
            ))
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 1024];

        loop {
            let bytes_read = file.read(&mut buffer).await.map_err(|err| {
                Error::from(anyhow!(
                    "error when reading file {file_name} when copying from pod {name}: {err}"
                ))
            })?;
            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
        }

        let actual_hash = hex::encode(hasher.finalize());

        // compute sha256sum of remote copied file
        let file_path = from.as_ref().to_string_lossy();
        let sha256sum = self
            .pod_exec(namespace, name, vec!["sha256sum", &file_path])
            .await?
            .map_err(|err| Error::from(anyhow!("error when checking file integrity when copying {file_name} from pod {name}: status {}: {}", err.0, err.1)))?;
        let expected_hash = sha256sum
            .split_whitespace()
            .next()
            .expect("sha256sum output should be in the form `hash<spaces>filename`");

        // check integrity
        if actual_hash != expected_hash {
            return Err(Error::from(anyhow!(
                "file {file_name} copy failed, expected sha256sum of {} got {} when copying file from {name}",
                expected_hash,
                actual_hash
            )));
        }

        Ok(())
    }

    async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()> {
        let pods = Api::<Pod>::namespaced(self.client.clone(), namespace);

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

    async fn create_service(
        &self,
        namespace: &str,
        name: &str,
        spec: ServiceSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<Service> {
        let services = Api::<Service>::namespaced(self.client.clone(), namespace);

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
}
