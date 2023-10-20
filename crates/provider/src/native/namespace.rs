use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
};

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{future::try_join_all, try_join};
use support::{fs::FileSystem, process::ProcessManager};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::{
    helpers::create_process_with_log_tasks,
    node::{NativeNode, NativeNodeInner},
    provider::WeakNativeProvider,
};
use crate::{
    constants::{NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_SCRIPTS_DIR},
    types::{GenerateFileCommand, GenerateFilesOptions, RunCommandOptions, SpawnNodeOptions},
    DynNode, ProviderError, ProviderNamespace, ProviderNode,
};

#[derive(Clone)]
pub(super) struct NativeNamespace<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub(super) name: String,
    pub(super) base_dir: PathBuf,
    pub(super) inner: Arc<RwLock<NativeNamespaceInner<FS, PM>>>,
    pub(super) filesystem: FS,
    pub(super) process_manager: PM,
    pub(super) provider: WeakNativeProvider<FS, PM>,
}

pub(super) struct NativeNamespaceInner<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub(super) nodes: HashMap<String, NativeNode<FS, PM>>,
}

#[derive(Clone)]
pub(super) struct WeakNativeNamespace<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone,
    PM: ProcessManager + Send + Sync + Clone,
{
    pub(super) inner: Weak<RwLock<NativeNamespaceInner<FS, PM>>>,
}

#[async_trait]
impl<FS, PM> ProviderNamespace for NativeNamespace<FS, PM>
where
    FS: FileSystem + Send + Sync + Clone + 'static,
    PM: ProcessManager + Send + Sync + Clone + 'static,
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

    async fn spawn_node(&self, options: SpawnNodeOptions) -> Result<DynNode, ProviderError> {
        let mut inner = self.inner.write().await;

        if inner.nodes.contains_key(&options.name) {
            return Err(ProviderError::DuplicatedNodeName(options.name));
        }

        // create node directories and filepaths
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

        // Created needed paths
        let ops_fut: Vec<_> = options
            .created_paths
            .iter()
            .map(|created_path| {
                self.filesystem.create_dir_all(format!(
                    "{}{}",
                    &base_dir.to_string_lossy(),
                    created_path.to_string_lossy()
                ))
            })
            .collect();
        try_join_all(ops_fut).await?;

        // copy injected files
        let ops_fut: Vec<_> = options
            .injected_files
            .iter()
            .map(|file| {
                self.filesystem.copy(
                    &file.local_path,
                    format!("{}{}", base_dir_raw, file.remote_path.to_string_lossy()),
                )
            })
            .collect();
        try_join_all(ops_fut).await?;

        let (process, stdout_reading_handle, stderr_reading_handle, log_writing_handle) =
            create_process_with_log_tasks(
                &options.name,
                &options.program,
                &options.args,
                &options.env,
                &log_path,
                self.filesystem.clone(),
                self.process_manager.clone(),
            )
            .await?;

        // create node structure holding state
        let node = NativeNode {
            name: options.name.clone(),
            program: options.program,
            args: options.args,
            env: options.env,
            base_dir,
            config_dir,
            data_dir,
            scripts_dir,
            log_path,
            filesystem: self.filesystem.clone(),
            process_manager: self.process_manager.clone(),
            namespace: WeakNativeNamespace {
                inner: Arc::downgrade(&self.inner),
            },
            inner: Arc::new(RwLock::new(NativeNodeInner {
                process,
                stdout_reading_handle,
                stderr_reading_handle,
                log_writing_handle,
            })),
        };

        // store node inside namespace
        inner.nodes.insert(options.name, node.clone());

        Ok(Arc::new(node))
    }

    async fn generate_files(&self, options: GenerateFilesOptions) -> Result<(), ProviderError> {
        // we spawn a node doing nothing but looping so we can execute our commands
        let temp_node = self
            .spawn_node(SpawnNodeOptions {
                name: format!("temp_{}", Uuid::new_v4()),
                program: "bash".to_string(),
                args: vec!["-c".to_string(), "while :; do sleep 1; done".to_string()],
                env: vec![],
                injected_files: options.injected_files,
                created_paths: vec![],
            })
            .await?;

        for GenerateFileCommand {
            program,
            args,
            env,
            local_output_path,
        } in options.commands
        {
            // TODO: move to logger
            // println!("{:#?}, {:#?}", command, args);
            // println!("{:#?}", self.base_dir.to_string_lossy());
            // println!("{:#?}", local_output_path.as_os_str());
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
        // no static setup exists for native provider
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        // we need to clone nodes (behind an Arc, so cheaply) to avoid deadlock between the inner.write lock and the node.destroy
        // method acquiring a lock the namespace to remove the node from the nodes hashmap.
        let nodes: Vec<NativeNode<FS, PM>> =
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

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, str::FromStr, time::Duration};

    use support::{
        fs::in_memory::{InMemoryFile, InMemoryFileSystem},
        process::fake::{DynamicStreamValue, FakeProcessManager, FakeProcessState, StreamValue},
    };
    use tokio::time::{sleep, timeout};

    use super::*;
    use crate::{native::provider::NativeProvider, types::TransferedFile, Provider};

    #[tokio::test]
    async fn spawn_node_should_creates_a_new_node_correctly() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
            (
                OsString::from_str("/file1").unwrap(),
                InMemoryFile::file("My file 1"),
            ),
            (
                OsString::from_str("/file2").unwrap(),
                InMemoryFile::file("My file 2"),
            ),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        let node = namespace
            .spawn_node(
                SpawnNodeOptions::new("mynode", "/path/to/my/node_binary")
                    .args(vec![
                        "-flag1",
                        "--flag2",
                        "--option1=value1",
                        "-option2=value2",
                        "--option3 value3",
                        "-option4 value4",
                    ])
                    .env(vec![
                        ("MY_VAR_1", "MY_VALUE_1"),
                        ("MY_VAR_2", "MY_VALUE_2"),
                        ("MY_VAR_3", "MY_VALUE_3"),
                    ])
                    .injected_files(vec![
                        TransferedFile::new("/file1", "/cfg/file1"),
                        TransferedFile::new("/file2", "/data/file2"),
                    ]),
            )
            .await
            .unwrap();

        // ensure node directories are created
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.base_dir().as_os_str()));
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.config_dir().as_os_str()));
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.data_dir().as_os_str()));
        assert!(fs
            .files
            .read()
            .await
            .contains_key(node.scripts_dir().as_os_str()));

        // ensure injected files are presents
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!("{}/file1", node.config_dir().to_string_lossy()))
                        .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 1"
        );
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!("{}/file2", node.data_dir().to_string_lossy()))
                        .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 2"
        );

        // ensure only one process exists
        assert_eq!(pm.count().await, 1);

        // retrieve the process
        let processes = pm.processes().await;
        let process = processes.first().unwrap();

        // ensure process has correct state
        assert!(matches!(process.state().await, FakeProcessState::Running));

        // ensure process is passed correct args
        assert!(process
            .args
            .contains(&OsString::from_str("-flag1").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--flag2").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--option1=value1").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("-option2=value2").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("--option3 value3").unwrap()));
        assert!(process
            .args
            .contains(&OsString::from_str("-option4 value4").unwrap()));

        // ensure process has correct environment
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_1").unwrap(),
            OsString::from_str("MY_VALUE_1").unwrap()
        )));
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_2").unwrap(),
            OsString::from_str("MY_VALUE_2").unwrap()
        )));
        assert!(process.envs.contains(&(
            OsString::from_str("MY_VAR_3").unwrap(),
            OsString::from_str("MY_VALUE_3").unwrap()
        )));

        // ensure log file is created and logs are written and keep being written for some time
        pm.advance_by(1).await;
        let expected = ["Line 1\n", "Line 1\nLine 2\n", "Line 1\nLine 2\nLine 3\n"];
        let mut index = 0;
        timeout(Duration::from_secs(3), async {
            loop {
                // if we reach the expected len, all logs have been emited correctly in order
                if index == expected.len() {
                    break;
                }

                // check if there is some existing file with contents
                if let Some(contents) = fs
                    .files
                    .read()
                    .await
                    .get(node.log_path().as_os_str())
                    .map(|file| file.contents().unwrap())
                {
                    // if the contents correspond to what we expect, we continue to check the next expected thing and simulate cpu cycle
                    if contents == expected[index] {
                        index += 1;
                        pm.advance_by(1).await;
                    }
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // ensure node is present in namespace
        assert_eq!(namespace.nodes().await.len(), 1);
        assert!(namespace.nodes().await.get(node.name()).is_some());
    }

    #[tokio::test]
    async fn spawn_node_should_returns_an_error_if_a_node_already_exists_with_this_name() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "./testing/dummy_node"))
            .await
            .unwrap();

        let result = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "./testing/dummy_node"))
            .await;

        // we must match here because Arc<dyn Node + Send + Sync> doesn't implements Debug, so unwrap_err is not an option
        match result {
            Ok(_) => unreachable!(),
            Err(err) => assert_eq!(err.to_string(), "Duplicated node name: mynode"),
        };
    }

    #[tokio::test]
    async fn spawn_node_should_returns_an_error_spawning_process_failed() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::new());
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // force error
        pm.spawn_should_error(std::io::ErrorKind::TimedOut).await;

        let result = namespace
            .spawn_node(SpawnNodeOptions::new("mynode", "./testing/dummy_node"))
            .await;

        // we must match here because Arc<dyn Node + Send + Sync> doesn't implements Debug, so unwrap_err is not an option
        match result {
            Ok(_) => unreachable!(),
            Err(err) => assert_eq!(err.to_string(), "Failed to spawn node 'mynode': timed out"),
        };
    }

    #[tokio::test]
    async fn generate_files_should_create_files_at_the_correct_locations_using_given_commands() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([
            (
                OsString::from_str("echo").unwrap(),
                vec![StreamValue::DynamicStdout(DynamicStreamValue::new(
                    |_, args, _| format!("{}\n", args.first().unwrap().to_string_lossy()),
                ))],
            ),
            (
                OsString::from_str("sh").unwrap(),
                vec![StreamValue::DynamicStdout(DynamicStreamValue::new(
                    |_, _, envs| envs.first().unwrap().1.to_string_lossy().to_string(),
                ))],
            ),
        ]));
        let provider = NativeProvider::new(fs.clone(), pm).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        namespace
            .generate_files(GenerateFilesOptions::new(vec![
                GenerateFileCommand::new("echo", "/myfile1").args(vec!["My file 1"]),
                GenerateFileCommand::new("sh", "/myfile2")
                    .args(vec!["-c", "echo -n $MY_CONTENT"])
                    .env(vec![("MY_CONTENT", "My file 2")]),
            ]))
            .await
            .unwrap();

        // ensure files have been generated correctly to right location
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!(
                        "{}/myfile1",
                        namespace.base_dir().to_string_lossy()
                    ))
                    .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 1\n"
        );
        assert_eq!(
            fs.files
                .read()
                .await
                .get(
                    &OsString::from_str(&format!(
                        "{}/myfile2",
                        namespace.base_dir().to_string_lossy()
                    ))
                    .unwrap()
                )
                .unwrap()
                .contents()
                .unwrap(),
            "My file 2"
        );

        // ensure temporary node has been destroyed
        assert_eq!(namespace.nodes().await.len(), 0);
    }

    #[tokio::test]
    async fn destroy_should_destroy_all_namespace_nodes_and_namespace_itself() {
        let fs = InMemoryFileSystem::new(HashMap::from([
            (OsString::from_str("/").unwrap(), InMemoryFile::dir()),
            (OsString::from_str("/tmp").unwrap(), InMemoryFile::dir()),
        ]));
        let pm = FakeProcessManager::new(HashMap::from([(
            OsString::from_str("/path/to/my/node_binary").unwrap(),
            vec![
                StreamValue::Stdout("Line 1\n".to_string()),
                StreamValue::Stdout("Line 2\n".to_string()),
                StreamValue::Stdout("Line 3\n".to_string()),
            ],
        )]));
        let provider = NativeProvider::new(fs.clone(), pm.clone()).tmp_dir("/tmp");
        let namespace = provider.create_namespace().await.unwrap();

        // spawn 2 dummy nodes to populate namespace
        namespace
            .spawn_node(SpawnNodeOptions::new("mynode1", "/path/to/my/node_binary"))
            .await
            .unwrap();
        namespace
            .spawn_node(SpawnNodeOptions::new("mynode2", "/path/to/my/node_binary"))
            .await
            .unwrap();

        // ensure nodes are presents
        assert_eq!(namespace.nodes().await.len(), 2);

        namespace.destroy().await.unwrap();

        // ensure nodes are destroyed
        assert_eq!(namespace.nodes().await.len(), 0);

        // ensure no running process exists
        assert_eq!(pm.processes().await.len(), 0);

        // ensure namespace is destroyed
        assert_eq!(provider.namespaces().await.len(), 0);
    }
}
