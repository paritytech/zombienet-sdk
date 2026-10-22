//! A provider node for tests: logs that can be pushed, and a scripted status.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use configuration::types::AssetLocation;
use provider::{types::*, ProviderError, ProviderNode};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct MockNode {
    logs: Arc<Mutex<Vec<String>>>,
    /// What `status()` answers, in order; the last answer repeats.
    #[serde(skip)]
    statuses: Mutex<VecDeque<Result<ProcessStatus, String>>>,
}

impl MockNode {
    pub(crate) fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(vec![])),
            statuses: Mutex::new(VecDeque::new()),
        }
    }

    /// Script what `status()` reports, see `statuses`.
    pub(crate) fn with_statuses(self, statuses: Vec<Result<ProcessStatus, &str>>) -> Self {
        *self.statuses.lock().unwrap() = statuses
            .into_iter()
            .map(|s| s.map_err(String::from))
            .collect();
        self
    }

    pub(crate) fn logs_push(&self, lines: Vec<impl Into<String>>) {
        self.logs
            .lock()
            .unwrap()
            .extend(lines.into_iter().map(|l| l.into()));
    }
}

#[async_trait]
impl ProviderNode for MockNode {
    fn name(&self) -> &str {
        todo!()
    }

    fn args(&self) -> Vec<&str> {
        todo!()
    }

    fn base_dir(&self) -> &PathBuf {
        todo!()
    }

    fn config_dir(&self) -> &PathBuf {
        todo!()
    }

    fn data_dir(&self) -> &PathBuf {
        todo!()
    }

    fn relay_data_dir(&self) -> &PathBuf {
        todo!()
    }

    fn scripts_dir(&self) -> &PathBuf {
        todo!()
    }

    fn log_path(&self) -> &PathBuf {
        todo!()
    }

    fn log_cmd(&self) -> String {
        todo!()
    }

    fn path_in_node(&self, _file: &Path) -> PathBuf {
        todo!()
    }

    async fn status(&self) -> Result<ProcessStatus, ProviderError> {
        let mut statuses = self.statuses.lock().unwrap();
        let next = if statuses.len() > 1 {
            statuses.pop_front()
        } else {
            statuses.front().cloned()
        };
        match next {
            Some(Ok(status)) => Ok(status),
            Some(Err(err)) => Err(ProviderError::InvalidConfig(err)),
            None => Ok(ProcessStatus::Running { ready: None }),
        }
    }

    async fn logs(&self) -> Result<String, ProviderError> {
        Ok(self.logs.lock().unwrap().join("\n"))
    }

    async fn dump_logs(&self, _local_dest: PathBuf) -> Result<(), ProviderError> {
        todo!()
    }

    async fn run_command(
        &self,
        _options: RunCommandOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        todo!()
    }

    async fn run_script(
        &self,
        _options: RunScriptOptions,
    ) -> Result<ExecutionResult, ProviderError> {
        todo!()
    }

    async fn send_file(
        &self,
        _local_file_path: &Path,
        _remote_file_path: &Path,
        _mode: &str,
    ) -> Result<(), ProviderError> {
        todo!()
    }

    async fn receive_file(
        &self,
        _remote_file_path: &Path,
        _local_file_path: &Path,
    ) -> Result<(), ProviderError> {
        todo!()
    }

    async fn pause(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn resume(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn restart(&self, _after: Option<Duration>) -> Result<(), ProviderError> {
        todo!()
    }

    async fn restart_with(
        &self,
        _assets: &[AssetLocation],
        _cmd: &str,
        _args: &[String],
        _after: Option<Duration>,
    ) -> Result<(), ProviderError> {
        todo!()
    }

    async fn destroy(&self) -> Result<(), ProviderError> {
        todo!()
    }

    async fn snapshot_db(&self, _: bool) -> Result<InnerSnapshotDb, ProviderError> {
        todo!()
    }
}
