use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use support::fs::FileSystem;
use tokio::sync::RwLock;
use tracing::{trace, warn};
use uuid::Uuid;

use super::node::{NativeNode, NativeNodeOptions};
use crate::{
    constants::NAMESPACE_PREFIX,
    native::{node::DeserializableNativeNodeOptions, provider},
    shared::helpers::extract_execution_result,
    types::{
        GenerateFileCommand, GenerateFilesOptions, ProviderCapabilities, RunCommandOptions,
        SpawnNodeOptions,
    },
    DynNode, NativeProvider, ProviderError, ProviderNamespace, ProviderNode,
};

pub(super) struct NativeNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone,
{
    weak: Weak<NativeNamespace<FS>>,
    name: String,
    provider: Weak<NativeProvider<FS>>,
    base_dir: PathBuf,
    capabilities: ProviderCapabilities,
    filesystem: FS,
    pub(super) nodes: RwLock<HashMap<String, Arc<NativeNode<FS>>>>,
}

impl<FS> NativeNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    pub(super) async fn new(
        provider: &Weak<NativeProvider<FS>>,
        tmp_dir: &PathBuf,
        capabilities: &ProviderCapabilities,
        filesystem: &FS,
        custom_base_dir: Option<&Path>,
    ) -> Result<Arc<Self>, ProviderError> {
        let name = format!("{}{}", NAMESPACE_PREFIX, Uuid::new_v4());
        let base_dir = if let Some(custom_base_dir) = custom_base_dir {
            if !filesystem.exists(custom_base_dir).await {
                filesystem.create_dir_all(custom_base_dir).await?;
            } else {
                warn!(
                    "⚠️ Using and existing directory {} as base dir",
                    custom_base_dir.to_string_lossy()
                );
            }
            PathBuf::from(custom_base_dir)
        } else {
            let base_dir = PathBuf::from_iter([tmp_dir, &PathBuf::from(&name)]);
            filesystem.create_dir(&base_dir).await?;
            base_dir
        };

        Ok(Arc::new_cyclic(|weak| NativeNamespace {
            weak: weak.clone(),
            provider: provider.clone(),
            name,
            base_dir,
            capabilities: capabilities.clone(),
            filesystem: filesystem.clone(),
            nodes: RwLock::new(HashMap::new()),
        }))
    }

    pub(super) async fn attach_to_live(
        provider: &Weak<NativeProvider<FS>>,
        capabilities: &ProviderCapabilities,
        filesystem: &FS,
        custom_base_dir: &Path,
        name: &str,
    ) -> Result<Arc<Self>, ProviderError> {
        let base_dir = custom_base_dir.to_path_buf();

        Ok(Arc::new_cyclic(|weak| NativeNamespace {
            weak: weak.clone(),
            provider: provider.clone(),
            name: name.to_string(),
            base_dir,
            capabilities: capabilities.clone(),
            filesystem: filesystem.clone(),
            nodes: RwLock::new(HashMap::new()),
        }))
    }
}

#[async_trait]
impl<FS> ProviderNamespace for NativeNamespace<FS>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    fn capabilities(&self) -> &ProviderCapabilities {
        &self.capabilities
    }

    fn provider_name(&self) -> &str {
        provider::PROVIDER_NAME
    }

    async fn nodes(&self) -> HashMap<String, DynNode> {
        self.nodes
            .read()
            .await
            .iter()
            .map(|(name, node)| (name.clone(), node.clone() as DynNode))
            .collect()
    }

    async fn get_node_available_args(
        &self,
        (command, _image): (String, Option<String>),
    ) -> Result<String, ProviderError> {
        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(format!("temp-{}", Uuid::new_v4()), "bash".to_string())
                    .args(vec!["-c", "while :; do sleep 1; done"]),
            )
            .await?;

        let available_args_output = temp_node
            .run_command(RunCommandOptions::new(command.clone()).args(vec!["--help"]))
            .await?
            .map_err(|(_exit, status)| {
                ProviderError::NodeAvailableArgsError("".to_string(), command, status)
            })?;

        temp_node.destroy().await?;

        Ok(available_args_output)
    }

    async fn spawn_node(&self, options: &SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        trace!("spawn node options {options:?}");

        let node = NativeNode::new(NativeNodeOptions {
            namespace: &self.weak,
            namespace_base_dir: &self.base_dir,
            name: &options.name,
            program: &options.program,
            args: &options.args,
            env: &options.env,
            startup_files: &options.injected_files,
            created_paths: &options.created_paths,
            db_snapshot: options.db_snapshot.as_ref(),
            filesystem: &self.filesystem,
            node_log_path: options.node_log_path.as_ref(),
        })
        .await?;

        self.nodes
            .write()
            .await
            .insert(options.name.clone(), node.clone());

        Ok(node)
    }

    async fn spawn_node_from_json(
        &self,
        json_value: &serde_json::Value,
    ) -> Result<DynNode, ProviderError> {
        let deserializable: DeserializableNativeNodeOptions =
            serde_json::from_value(json_value.clone())?;
        let options = NativeNodeOptions::from_deserializable(
            &deserializable,
            &self.weak,
            &self.base_dir,
            &self.filesystem,
        );

        let pid = json_value
            .get("process_handle")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| ProviderError::InvalidConfig("Missing pid field".to_string()))?
            as i32;
        let node = NativeNode::attach_to_live(options, pid).await?;

        self.nodes
            .write()
            .await
            .insert(node.name().to_string(), node.clone());

        Ok(node)
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        let node_name = if let Some(name) = options.temp_name {
            name
        } else {
            format!("temp-{}", Uuid::new_v4())
        };

        // we spawn a node doing nothing but looping so we can execute our commands
        let temp_node = self
            .spawn_node(
                &SpawnNodeOptions::new(node_name, "bash".to_string())
                    .args(vec!["-c", "while :; do sleep 1; done"])
                    .injected_files(options.injected_files),
            )
            .await?;

        for GenerateFileCommand {
            program,
            args,
            env,
            local_output_path,
        } in options.commands
        {
            trace!(
                "🏗  building file {:?} in path {} with command {} {}",
                local_output_path.as_os_str(),
                self.base_dir.to_string_lossy(),
                program,
                args.join(" ")
            );
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

            let contents = extract_execution_result(
                &temp_node,
                RunCommandOptions { program, args, env },
                options.expected_path.as_ref(),
            )
            .await?;
            self.filesystem
                .write(local_output_full_path, contents)
                .await
                .map_err(|err| ProviderError::FileGenerationFailed(err.into()))?;
        }

        temp_node.destroy().await
    }

    async fn static_setup(&self) -> Result<(), ProviderError> {
        // no static setup exists for native provider
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        let mut names = vec![];

        for node in self.nodes.read().await.values() {
            node.abort()
                .await
                .map_err(|err| ProviderError::DestroyNodeFailed(node.name().to_string(), err))?;
            names.push(node.name().to_string());
        }

        let mut nodes = self.nodes.write().await;
        for name in names {
            nodes.remove(&name);
        }

        if let Some(provider) = self.provider.upgrade() {
            provider.namespaces.write().await.remove(&self.name);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use support::fs::local::LocalFileSystem;

    use super::*;
    use crate::{
        types::{GenerateFileCommand, GenerateFilesOptions},
        NativeProvider, Provider,
    };

    fn unique_temp_dir() -> PathBuf {
        let mut base = std::env::temp_dir();
        base.push(format!("znet_native_ns_test_{}", uuid::Uuid::new_v4()));
        base
    }

    #[tokio::test]
    async fn generate_files_uses_expected_path_when_provided() {
        let fs = LocalFileSystem;
        let provider = NativeProvider::new(fs.clone());
        let base_dir = unique_temp_dir();
        // Namespace builder will create directory if needed
        let ns = provider
            .create_namespace_with_base_dir(&base_dir)
            .await
            .expect("namespace should be created");

        // Create a unique on-host path that the native node will write to
        let expected_path =
            std::env::temp_dir().join(format!("znet_expected_{}.json", uuid::Uuid::new_v4()));

        // Command will write JSON into expected_path; stdout will be something else to ensure we don't read it
        let program = "bash".to_string();
        let script = format!(
            "echo -n '{{\"hello\":\"world\"}}' > {} && echo should_not_be_used",
            expected_path.to_string_lossy()
        );
        let args: Vec<String> = vec!["-lc".into(), script];

        let out_name = PathBuf::from("result_expected.json");
        let cmd = GenerateFileCommand::new(program, out_name.clone()).args(args);
        let options = GenerateFilesOptions::new(vec![cmd], None, Some(expected_path.clone()));

        ns.generate_files(options)
            .await
            .expect("generation should succeed");

        // Read produced file from namespace base_dir
        let produced_path = base_dir.join(out_name);
        let produced = fs
            .read_to_string(&produced_path)
            .await
            .expect("should read produced file");
        assert_eq!(produced, "{\"hello\":\"world\"}");
    }

    #[tokio::test]
    async fn generate_files_uses_stdout_when_expected_path_absent() {
        let fs = LocalFileSystem;
        let provider = NativeProvider::new(fs.clone());
        let base_dir = unique_temp_dir();
        let ns = provider
            .create_namespace_with_base_dir(&base_dir)
            .await
            .expect("namespace should be created");

        // Command prints to stdout only
        let program = "bash".to_string();
        let args: Vec<String> = vec!["-lc".into(), "echo -n 42".into()];

        let out_name = PathBuf::from("result_stdout.txt");
        let cmd = GenerateFileCommand::new(program, out_name.clone()).args(args);
        let options = GenerateFilesOptions::new(vec![cmd], None, None);

        ns.generate_files(options)
            .await
            .expect("generation should succeed");

        let produced_path = base_dir.join(out_name);
        let produced = fs
            .read_to_string(&produced_path)
            .await
            .expect("should read produced file");
        assert_eq!(produced, "42");
    }
}
