use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{future::try_join_all, try_join};
use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, PodSpec, ResourceRequirements, Volume,
        VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
};
use support::fs::FileSystem;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use super::{
    client::KubernetesClient,
    node::{KubernetesNode, KubernetesNodeInner},
    provider::WeakKubernetesProvider,
};
use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_SCRIPTS_DIR},
    shared::helpers::{create_log_writing_task, create_stream_polling_task},
    types::{GenerateFileCommand, GenerateFilesOptions, RunCommandOptions, SpawnNodeOptions},
    DynNode, ProviderError, ProviderNamespace, ProviderNode,
};

#[derive(Clone)]
pub(super) struct KubernetesNamespace<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) name: String,
    pub(super) base_dir: PathBuf,
    pub(super) inner: Arc<RwLock<KubernetesNamespaceInner<FS, KC>>>,
    pub(super) filesystem: FS,
    pub(super) client: KC,
    pub(super) provider: WeakKubernetesProvider<FS, KC>,
}

#[derive(Clone)]
pub(super) struct KubernetesNamespaceInner<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) nodes: HashMap<String, KubernetesNode<FS, KC>>,
}

#[derive(Clone)]
pub(super) struct WeakKubernetesNamespace<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone,
    KC: KubernetesClient<FS> + Send + Sync + Clone,
{
    pub(super) inner: Weak<RwLock<KubernetesNamespaceInner<FS, KC>>>,
}

#[async_trait]
impl<FS, KC> ProviderNamespace for KubernetesNamespace<FS, KC>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    KC: KubernetesClient<FS> + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    async fn nodes(&self) -> HashMap<String, DynNode> {
        self.inner
            .read()
            .await
            .nodes
            .clone()
            .into_iter()
            .map(|(id, node)| (id, Arc::new(node) as DynNode))
            .collect()
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        let mut inner = self.inner.write().await;

        if inner.nodes.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name.clone()));
        }

        // create node directories and filepaths
        // TODO: remove duplication between providers
        let base_dir_raw = format!("{}/{}", &self.base_dir.to_string_lossy(), &options.name);
        let base_dir = PathBuf::from(&base_dir_raw);
        let log_path = PathBuf::from(format!("{}/{}.log", base_dir_raw, &options.name));
        let config_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_CONFIG_DIR));
        let data_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_DATA_DIR));
        let scripts_dir = PathBuf::from(format!("{}{}", base_dir_raw, NODE_SCRIPTS_DIR));
        // NOTE: in native this base path already exist
        self.filesystem.create_dir_all(&base_dir).await?;
        try_join!(
            self.filesystem.create_dir(&config_dir),
            self.filesystem.create_dir(&data_dir),
            self.filesystem.create_dir(&scripts_dir),
        )?;

        // creat k8s pod
        let manifest = self
            .client
            .create_pod(
                &self.name,
                &options.name,
                PodSpec {
                    hostname: Some(options.name.to_string()),
                    containers: vec![Container {
                        name: options.name.clone(),
                        image: options.image.clone(),
                        image_pull_policy: Some("Always".to_string()),
                        command: Some(
                            [
                                vec!["/zombie-wrapper.sh".to_string(), options.program.clone()],
                                options.args.clone(),
                            ]
                            .concat(),
                        ),
                        env: Some(
                            options
                                .env
                                .iter()
                                .map(|(name, value)| EnvVar {
                                    name: name.clone(),
                                    value: Some(value.clone()),
                                    value_from: None,
                                })
                                .collect(),
                        ),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "zombie-wrapper-volume".to_string(),
                            mount_path: "/zombie-wrapper.sh".to_string(),
                            sub_path: Some("zombie-wrapper.sh".to_string()),
                            ..Default::default()
                        }]),
                        resources: Some(ResourceRequirements {
                            limits: if options.resources.is_some() {
                                let mut limits = BTreeMap::new();

                                if let Some(limit_cpu) = options
                                    .resources
                                    .clone()
                                    .expect("safe to unwrap")
                                    .limit_cpu()
                                {
                                    limits.insert(
                                        "cpu".to_string(),
                                        Quantity(limit_cpu.as_str().to_string()),
                                    );
                                }
                                if let Some(limit_memory) = options
                                    .resources
                                    .clone()
                                    .expect("safe to unwrap")
                                    .request_memory()
                                {
                                    limits.insert(
                                        "memory".to_string(),
                                        Quantity(limit_memory.as_str().to_string()),
                                    );
                                }

                                if !limits.is_empty() {
                                    Some(limits)
                                } else {
                                    None
                                }
                            } else {
                                None
                            },
                            requests: if options.resources.is_some() {
                                let mut request = BTreeMap::new();

                                if let Some(request_cpu) = options
                                    .resources
                                    .clone()
                                    .expect("safe to unwrap")
                                    .request_cpu()
                                {
                                    request.insert(
                                        "cpu".to_string(),
                                        Quantity(request_cpu.as_str().to_string()),
                                    );
                                }
                                if let Some(request_memory) = options
                                    .resources
                                    .clone()
                                    .expect("safe to unwrap")
                                    .request_memory()
                                {
                                    request.insert(
                                        "memory".to_string(),
                                        Quantity(request_memory.as_str().to_string()),
                                    );
                                }

                                if !request.is_empty() {
                                    Some(request)
                                } else {
                                    None
                                }
                            } else {
                                None
                            },
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    volumes: Some(vec![
                        Volume {
                            name: "cfg".to_string(),
                            ..Default::default()
                        },
                        Volume {
                            name: "data".to_string(),
                            ..Default::default()
                        },
                        Volume {
                            name: "relay-data".to_string(),
                            ..Default::default()
                        },
                        Volume {
                            name: "zombie-wrapper-volume".to_string(),
                            config_map: Some(ConfigMapVolumeSource {
                                name: Some("zombie-wrapper".to_string()),
                                default_mode: Some(0o755),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                },
                BTreeMap::from([("some".to_string(), "labels".to_string())]),
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to created pod {}", options.name),
                    err.into(),
                )
            })?;

        // store pod manifest
        self.filesystem
            .write(
                PathBuf::from_iter([&base_dir, &PathBuf::from("pod_manifest.yaml")]),
                serde_yaml::to_string(&manifest).map_err(|err| {
                    ProviderError::NodeSpawningFailed(
                        format!("failed to serialize pod manifest {}", options.name),
                        err.into(),
                    )
                })?,
            )
            .await?;

        // create paths
        let ops_fut: Vec<_> = options
            .created_paths
            .clone()
            .into_iter()
            .map(|created_path| {
                self.client.pod_exec(
                    &self.name,
                    &options.name,
                    vec!["mkdir", "-p", &created_path.to_string_lossy()]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect(),
                )
            })
            .collect();

        try_join_all(ops_fut).await.map_err(|err| {
            ProviderError::NodeSpawningFailed(
                format!("failed to create paths for pod {}", options.name),
                err.into(),
            )
        })?;

        // copy injected files
        let ops_fut: Vec<_> = options
            .injected_files
            .iter()
            .map(|file| {
                self.client.copy_to_pod(
                    &self.name,
                    &options.name,
                    &file.local_path,
                    &file.remote_path,
                    &file.mode,
                )
            })
            .collect();

        try_join_all(ops_fut).await.map_err(|err| {
            ProviderError::NodeSpawningFailed(
                format!("failed to copy injected files for pod {}", options.name),
                err.into(),
            )
        })?;

        // start process
        self.client
            .pod_exec(
                &self.name,
                &options.name,
                vec!["sh", "-c", "echo start > /tmp/zombiepipe"],
            )
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to start pod {} after spawning", options.name),
                    err.into(),
                )
            })?
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to start pod {} after spawning", options.name,),
                    anyhow!("command failed in container: status {}: {}", err.0, err.1),
                )
            })?;

        // create log stream
        let logs_stream = self
            .client
            .create_pod_logs_stream(&self.name, &options.name)
            .await
            .map_err(|err| {
                ProviderError::NodeSpawningFailed(
                    format!("failed to create log stream for pod {}", options.name),
                    err.into(),
                )
            })?;

        // handle log writing
        let (logs_tx, rx) = mpsc::channel(10);
        let log_reading_handle = create_stream_polling_task(logs_stream, logs_tx);
        let log_writing_handle =
            create_log_writing_task(rx, self.filesystem.clone(), log_path.clone());

        // create node structure holding state
        let node = KubernetesNode {
            name: options.name.clone(),
            namespace_name: self.name.clone(),
            program: options.program.clone(),
            args: options.args.clone(),
            base_dir,
            config_dir,
            data_dir,
            scripts_dir,
            log_path,
            filesystem: self.filesystem.clone(),
            client: self.client.clone(),
            namespace: WeakKubernetesNamespace {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(KubernetesNodeInner {
                log_reading_handle,
                log_writing_handle,
            })),
        };

        // store node inside namespace
        inner.nodes.insert(options.name.clone(), node.clone());

        Ok(Arc::new(node))
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        // run dummy command in new pod
        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(format!("temp-{}", Uuid::new_v4()), "cat".to_string())
                    .injected_files(options.injected_files)
                    .image(options.image.expect(
                        "image should be present when generating files with kubernetes provider",
                    )),
            )
            .await?;

        for GenerateFileCommand {
            program,
            args,
            env,
            local_output_path,
        } in options.commands
        {
            let local_output_full_path = format!(
                "{}{}{}",
                self.base_dir.to_string_lossy(),
                if local_output_path.starts_with("/") {
                    ""
                } else {
                    "/"
                },
                local_output_path.to_string_lossy()
            );

            match temp_node
                .run_command(RunCommandOptions { program, args, env })
                .await
                .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?
            {
                Ok(contents) => self
                    .filesystem
                    .write(local_output_full_path, contents)
                    .await
                    .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?,
                Err((_, msg)) => Err(ProviderError::FileGenerationFailed(anyhow!("{msg}")))?,
            };
        }

        temp_node.destroy().await
    }

    async fn static_setup(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        // we need to clone nodes (behind an Arc, so cheaply) to avoid deadlock between the inner.write lock and the node.destroy
        // method acquiring a lock the namespace to remove the node from the nodes hashmap.
        let nodes: Vec<KubernetesNode<FS, KC>> =
            self.inner.write().await.nodes.values().cloned().collect();
        for node in nodes.iter() {
            node.destroy().await?;
        }

        // remove namespace from provider
        if let Some(provider) = self.provider.inner.upgrade() {
            provider.write().await.namespaces.remove(&self.name);
        }

        Ok(())
    }
}
