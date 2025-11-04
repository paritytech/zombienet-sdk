use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::anyhow;
use fancy_regex::Regex;
use glob_match::glob_match;
use prom_metrics_parser::MetricMap;
use provider::DynNode;
use serde::{Serialize, Serializer};
use subxt::{backend::rpc::RpcClient, OnlineClient};
use support::net::{skip_err_while_waiting, wait_ws_ready};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::{network_spec::node::NodeSpec, tx_helper::client::get_client_from_url};

type BoxedClosure = Box<dyn Fn(&str) -> Result<bool, anyhow::Error> + Send + Sync>;

#[derive(Error, Debug)]
pub enum NetworkNodeError {
    #[error("metric '{0}' not found!")]
    MetricNotFound(String),
}

#[derive(Clone, Serialize)]
pub struct NetworkNode {
    #[serde(serialize_with = "serialize_provider_node")]
    pub(crate) inner: DynNode,
    // TODO: do we need the full spec here?
    // Maybe a reduce set of values.
    pub(crate) spec: NodeSpec,
    pub(crate) name: String,
    pub(crate) ws_uri: String,
    pub(crate) multiaddr: String,
    pub(crate) prometheus_uri: String,
    #[serde(skip)]
    metrics_cache: Arc<RwLock<MetricMap>>,
    #[serde(skip)]
    is_running: Arc<AtomicBool>,
}

/// Result of waiting for a certain number of log lines to appear.
///
/// Indicates whether the log line count condition was met within the timeout period.
///
/// # Variants
/// - `TargetReached(count)` – The predicate condition was satisfied within the timeout.
///     * `count`: The number of matching log lines at the time of satisfaction.
/// - `TargetFailed(count)` – The condition was not met within the timeout.
///     * `count`: The final number of matching log lines at timeout expiration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLineCount {
    TargetReached(u32),
    TargetFailed(u32),
}

impl LogLineCount {
    pub fn success(&self) -> bool {
        match self {
            Self::TargetReached(..) => true,
            Self::TargetFailed(..) => false,
        }
    }
}

/// Configuration for controlling log line count waiting behavior.
///
/// Allows specifying a custom predicate on the number of matching log lines,
/// a timeout in seconds, and whether the system should wait the entire timeout duration.
///
/// # Fields
/// - `predicate`: A function that takes the current number of matching lines and
///   returns `true` if the condition is satisfied.
/// - `timeout_secs`: Maximum number of seconds to wait.
/// - `wait_until_timeout_elapses`: If `true`, the system will continue waiting
///   for the full timeout duration, even if the condition is already met early.
///   Useful when you need to verify sustained absence or stability (e.g., "ensure no new logs appear").
#[derive(Clone)]
pub struct LogLineCountOptions {
    pub predicate: Arc<dyn Fn(u32) -> bool + Send + Sync>,
    pub timeout: Duration,
    pub wait_until_timeout_elapses: bool,
}

impl LogLineCountOptions {
    pub fn new(
        predicate: impl Fn(u32) -> bool + 'static + Send + Sync,
        timeout: Duration,
        wait_until_timeout_elapses: bool,
    ) -> Self {
        Self {
            predicate: Arc::new(predicate),
            timeout,
            wait_until_timeout_elapses,
        }
    }

    pub fn no_occurences_within_timeout(timeout: Duration) -> Self {
        Self::new(|n| n == 0, timeout, true)
    }
}

// #[derive(Clone, Debug)]
// pub struct QueryMetricOptions {
//     use_cache: bool,
//     treat_not_found_as_zero: bool,
// }

// impl Default for QueryMetricOptions {
//     fn default() -> Self {
//         Self { use_cache: false, treat_not_found_as_zero: true }
//     }
// }

impl NetworkNode {
    /// Create a new NetworkNode
    pub(crate) fn new<T: Into<String>>(
        name: T,
        ws_uri: T,
        prometheus_uri: T,
        multiaddr: T,
        spec: NodeSpec,
        inner: DynNode,
    ) -> Self {
        Self {
            name: name.into(),
            ws_uri: ws_uri.into(),
            prometheus_uri: prometheus_uri.into(),
            inner,
            spec,
            multiaddr: multiaddr.into(),
            metrics_cache: Arc::new(Default::default()),
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    pub(crate) fn set_is_running(&self, is_running: bool) {
        self.is_running.store(is_running, Ordering::Release);
    }

    pub(crate) fn set_multiaddr(&mut self, multiaddr: impl Into<String>) {
        self.multiaddr = multiaddr.into();
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn args(&self) -> Vec<&str> {
        self.inner.args()
    }

    pub fn spec(&self) -> &NodeSpec {
        &self.spec
    }

    pub fn ws_uri(&self) -> &str {
        &self.ws_uri
    }

    pub fn multiaddr(&self) -> &str {
        self.multiaddr.as_ref()
    }

    // Subxt

    /// Get the rpc client for the node
    pub async fn rpc(&self) -> Result<RpcClient, subxt::Error> {
        get_client_from_url(&self.ws_uri).await
    }

    /// Get the [online client](subxt::client::OnlineClient) for the node
    #[deprecated = "Use `wait_client` instead."]
    pub async fn client<Config: subxt::Config>(
        &self,
    ) -> Result<OnlineClient<Config>, subxt::Error> {
        self.try_client().await
    }

    /// Try to connect to the node.
    ///
    /// Most of the time you only want to use [`NetworkNode::wait_client`] that waits for
    /// the node to appear before it connects to it. This function directly tries
    /// to connect to the node and returns an error if the node is not yet available
    /// at that point in time.
    ///
    /// Returns a [`OnlineClient`] on success.
    pub async fn try_client<Config: subxt::Config>(
        &self,
    ) -> Result<OnlineClient<Config>, subxt::Error> {
        get_client_from_url(&self.ws_uri).await
    }

    /// Wait until get the [online client](subxt::client::OnlineClient) for the node
    pub async fn wait_client<Config: subxt::Config>(
        &self,
    ) -> Result<OnlineClient<Config>, anyhow::Error> {
        debug!("wait_client ws_uri: {}", self.ws_uri());
        wait_ws_ready(self.ws_uri())
            .await
            .map_err(|e| anyhow!("Error awaiting http_client to ws be ready, err: {e}"))?;

        self.try_client()
            .await
            .map_err(|e| anyhow!("Can't create a subxt client, err: {e}"))
    }

    /// Wait until get the [online client](subxt::client::OnlineClient) for the node with a defined timeout
    pub async fn wait_client_with_timeout<Config: subxt::Config>(
        &self,
        timeout_secs: impl Into<u64>,
    ) -> Result<OnlineClient<Config>, anyhow::Error> {
        debug!("waiting until subxt client is ready");
        tokio::time::timeout(
            Duration::from_secs(timeout_secs.into()),
            self.wait_client::<Config>(),
        )
        .await?
    }

    // Commands

    /// Pause the node, this is implemented by pausing the
    /// actual process (e.g polkadot) with sending `SIGSTOP` signal
    pub async fn pause(&self) -> Result<(), anyhow::Error> {
        self.set_is_running(false);
        self.inner.pause().await?;
        Ok(())
    }

    /// Resume the node, this is implemented by resuming the
    /// actual process (e.g polkadot) with sending `SIGCONT` signal
    pub async fn resume(&self) -> Result<(), anyhow::Error> {
        self.set_is_running(true);
        self.inner.resume().await?;
        Ok(())
    }

    /// Restart the node using the same `cmd`, `args` and `env` (and same isolated dir)
    pub async fn restart(&self, after: Option<Duration>) -> Result<(), anyhow::Error> {
        self.set_is_running(false);
        self.inner.restart(after).await?;
        self.set_is_running(true);
        Ok(())
    }

    // Metrics assertions

    /// Get metric value 'by name' from Prometheus (exposed by the node)
    /// metric name can be:
    /// with prefix (e.g: 'polkadot_')
    /// with chain attribute (e.g: 'chain=rococo-local')
    /// without prefix and/or without chain attribute
    pub async fn reports(&self, metric_name: impl Into<String>) -> Result<f64, anyhow::Error> {
        let metric_name = metric_name.into();
        // force cache reload
        self.fetch_metrics().await?;
        // by default we treat not found as 0 (same in v1)
        self.metric(&metric_name, true).await
    }

    /// Assert on a metric value 'by name' from Prometheus (exposed by the node)
    /// metric name can be:
    /// with prefix (e.g: 'polkadot_')
    /// with chain attribute (e.g: 'chain=rococo-local')
    /// without prefix and/or without chain attribute
    ///
    /// We first try to assert on the value using the cached metrics and
    /// if not meet the criteria we reload the cache and check again
    pub async fn assert(
        &self,
        metric_name: impl Into<String>,
        value: impl Into<f64>,
    ) -> Result<bool, anyhow::Error> {
        let value: f64 = value.into();
        self.assert_with(metric_name, |v| v == value).await
    }

    /// Assert on a metric value using a given predicate.
    /// See [`NetworkNode::reports`] description for details on metric name.
    pub async fn assert_with(
        &self,
        metric_name: impl Into<String>,
        predicate: impl Fn(f64) -> bool,
    ) -> Result<bool, anyhow::Error> {
        let metric_name = metric_name.into();
        // reload metrics
        self.fetch_metrics().await?;
        let val = self.metric(&metric_name, true).await?;
        trace!("🔎 Current value {val} passed to the predicated?");
        Ok(predicate(val))
    }

    // Wait methods for metrics

    /// Wait until a metric value pass the `predicate`
    pub async fn wait_metric(
        &self,
        metric_name: impl Into<String>,
        predicate: impl Fn(f64) -> bool,
    ) -> Result<(), anyhow::Error> {
        let metric_name = metric_name.into();
        debug!("waiting until metric {metric_name} pass the predicate");
        loop {
            let res = self.assert_with(&metric_name, &predicate).await;
            match res {
                Ok(res) => {
                    if res {
                        return Ok(());
                    }
                },
                Err(e) => match e.downcast::<reqwest::Error>() {
                    Ok(io_err) => {
                        if !skip_err_while_waiting(&io_err) {
                            return Err(io_err.into());
                        }
                    },
                    Err(other) => {
                        match other.downcast::<NetworkNodeError>() {
                            Ok(node_err) => {
                                if !matches!(node_err, NetworkNodeError::MetricNotFound(_)) {
                                    return Err(node_err.into());
                                }
                            },
                            Err(other) => return Err(other),
                        };
                    },
                },
            }

            // sleep to not spam prometheus
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Wait until a metric value pass the `predicate`
    /// with a timeout (secs)
    pub async fn wait_metric_with_timeout(
        &self,
        metric_name: impl Into<String>,
        predicate: impl Fn(f64) -> bool,
        timeout_secs: impl Into<u64>,
    ) -> Result<(), anyhow::Error> {
        let metric_name = metric_name.into();
        let secs = timeout_secs.into();
        debug!("waiting until metric {metric_name} pass the predicate");
        let res = tokio::time::timeout(
            Duration::from_secs(secs),
            self.wait_metric(&metric_name, predicate),
        )
        .await;

        if let Ok(inner_res) = res {
            match inner_res {
                Ok(_) => Ok(()),
                Err(e) => Err(anyhow!("Error waiting for metric: {e}")),
            }
        } else {
            // timeout
            Err(anyhow!(
                "Timeout ({secs}), waiting for metric {metric_name} pass the predicate"
            ))
        }
    }

    // Logs

    /// Get the logs of the node
    /// TODO: do we need the `since` param, maybe we could be handy later for loop filtering
    pub async fn logs(&self) -> Result<String, anyhow::Error> {
        Ok(self.inner.logs().await?)
    }

    /// Wait until a the number of matching log lines is reach
    pub async fn wait_log_line_count(
        &self,
        pattern: impl Into<String>,
        is_glob: bool,
        count: usize,
    ) -> Result<(), anyhow::Error> {
        let pattern = pattern.into();
        let pattern_clone = pattern.clone();
        debug!("waiting until we find pattern {pattern} {count} times");
        let match_fn: BoxedClosure = if is_glob {
            Box::new(move |line: &str| Ok(glob_match(&pattern, line)))
        } else {
            let re = Regex::new(&pattern)?;
            Box::new(move |line: &str| re.is_match(line).map_err(|e| anyhow!(e.to_string())))
        };

        loop {
            let mut q = 0_usize;
            let logs = self.logs().await?;
            for line in logs.lines() {
                trace!("line is {line}");
                if match_fn(line)? {
                    trace!("pattern {pattern_clone} match in line {line}");
                    q += 1;
                    if q >= count {
                        return Ok(());
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Waits until the number of matching log lines satisfies a custom condition,
    /// optionally waiting for the entire duration of the timeout.
    ///
    /// This method searches log lines for a given substring or glob pattern,
    /// and evaluates the number of matching lines using a user-provided predicate function.
    /// Optionally, it can wait for the full timeout duration to ensure the condition
    /// holds consistently (e.g., for verifying absence of logs).
    ///
    /// # Arguments
    /// * `substring` - The substring or pattern to match within log lines.
    /// * `is_glob` - Whether to treat `substring` as a glob pattern (`true`) or a regex (`false`).
    /// * `options` - Configuration for timeout, match count predicate, and full-duration waiting.
    ///
    /// # Returns
    /// * `Ok(LogLineCount::TargetReached(n))` if the predicate was satisfied within the timeout,
    /// * `Ok(LogLineCount::TargetFails(n))` if the predicate was not satisfied in time,
    /// * `Err(e)` if an error occurred during log retrieval or matching.
    ///
    /// # Example
    /// ```rust
    /// # use std::{sync::Arc, time::Duration};
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem};
    /// # use zombienet_orchestrator::{Orchestrator, network::node::{NetworkNode, LogLineCountOptions}};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), anyhow::Error> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    /// #   let network = orchestrator.spawn(config).await?;
    /// let node = network.get_node("alice")?;
    /// // Wait (up to 10 seconds) until pattern occurs once
    /// let options = LogLineCountOptions {
    ///     predicate: Arc::new(|count| count == 1),
    ///     timeout: Duration::from_secs(10),
    ///     wait_until_timeout_elapses: false,
    /// };
    /// let result = node
    ///     .wait_log_line_count_with_timeout("error", false, options)
    ///     .await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn wait_log_line_count_with_timeout(
        &self,
        substring: impl Into<String>,
        is_glob: bool,
        options: LogLineCountOptions,
    ) -> Result<LogLineCount, anyhow::Error> {
        let substring = substring.into();
        debug!(
            "waiting until match lines count within {} seconds",
            options.timeout.as_secs_f64()
        );

        let start = tokio::time::Instant::now();

        let match_fn: BoxedClosure = if is_glob {
            Box::new(move |line: &str| Ok(glob_match(&substring, line)))
        } else {
            let re = Regex::new(&substring)?;
            Box::new(move |line: &str| re.is_match(line).map_err(|e| anyhow!(e.to_string())))
        };

        if options.wait_until_timeout_elapses {
            tokio::time::sleep(options.timeout).await;
        }

        let mut q;
        loop {
            q = 0_u32;
            let logs = self.logs().await?;
            for line in logs.lines() {
                if match_fn(line)? {
                    q += 1;

                    // If `wait_until_timeout_elapses` is set then check the condition just once at the
                    // end after the whole log file is processed. This is to address the cases when the
                    // predicate becomes true and false again.
                    // eg. expected exactly 2 matching lines are expected but 3 are present
                    if !options.wait_until_timeout_elapses && (options.predicate)(q) {
                        return Ok(LogLineCount::TargetReached(q));
                    }
                }
            }

            if start.elapsed() >= options.timeout {
                break;
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        if (options.predicate)(q) {
            Ok(LogLineCount::TargetReached(q))
        } else {
            Ok(LogLineCount::TargetFailed(q))
        }
    }

    async fn fetch_metrics(&self) -> Result<(), anyhow::Error> {
        let response = reqwest::get(&self.prometheus_uri).await?;
        let metrics = prom_metrics_parser::parse(&response.text().await?)?;
        let mut cache = self.metrics_cache.write().await;
        *cache = metrics;
        Ok(())
    }

    /// Query individual metric by name
    async fn metric(
        &self,
        metric_name: &str,
        treat_not_found_as_zero: bool,
    ) -> Result<f64, anyhow::Error> {
        let mut metrics_map = self.metrics_cache.read().await;
        if metrics_map.is_empty() {
            // reload metrics
            drop(metrics_map);
            self.fetch_metrics().await?;
            metrics_map = self.metrics_cache.read().await;
        }

        if let Some(val) = metrics_map.get(metric_name) {
            Ok(*val)
        } else if treat_not_found_as_zero {
            Ok(0_f64)
        } else {
            Err(NetworkNodeError::MetricNotFound(metric_name.into()).into())
        }
    }

    /// Waits given number of seconds until node reports that it is up and running, which
    /// is determined by metric 'process_start_time_seconds', which should appear,
    /// when node finished booting up.
    ///
    ///
    /// # Arguments
    /// * `timeout_secs` - The number of seconds to wait.
    ///
    /// # Returns
    /// * `Ok()` if the node is up before timeout occured.
    /// * `Err(e)` if timeout or other error occurred while waiting.
    pub async fn wait_until_is_up(
        &self,
        timeout_secs: impl Into<u64>,
    ) -> Result<(), anyhow::Error> {
        self.wait_metric_with_timeout("process_start_time_seconds", |b| b >= 1.0, timeout_secs)
            .await
            .map_err(|err| anyhow::anyhow!("{}: {:?}", self.name(), err))
    }
}

impl std::fmt::Debug for NetworkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkNode")
            .field("inner", &"inner_skipped")
            .field("spec", &self.spec)
            .field("name", &self.name)
            .field("ws_uri", &self.ws_uri)
            .field("prometheus_uri", &self.prometheus_uri)
            .finish()
    }
}

fn serialize_provider_node<S>(node: &DynNode, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    erased_serde::serialize(node.as_ref(), serializer)
}

// TODO: mock and impl more unit tests
#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use provider::{types::*, ProviderError, ProviderNode};

    use super::*;

    #[derive(Serialize)]
    struct MockNode {
        logs: Arc<Mutex<Vec<String>>>,
    }

    impl MockNode {
        fn new() -> Self {
            Self {
                logs: Arc::new(Mutex::new(vec![])),
            }
        }

        fn logs_push(&self, lines: Vec<impl Into<String>>) {
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

        async fn destroy(&self) -> Result<(), ProviderError> {
            todo!()
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_target_reached_immediately() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            "stub line 2",
            "system ready",
        ]);

        // Wait (up to 10 seconds) until pattern occurs once
        let options = LogLineCountOptions {
            predicate: Arc::new(|n| n == 1),
            timeout: Duration::from_secs(10),
            wait_until_timeout_elapses: false,
        };

        let log_line_count = mock_node
            .wait_log_line_count_with_timeout("system ready", false, options)
            .await?;

        assert!(matches!(log_line_count, LogLineCount::TargetReached(1)));

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_target_reached_after_delay() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            "stub line 2",
            "system ready",
        ]);

        // Wait (up to 4 seconds) until pattern occurs twice
        let options = LogLineCountOptions {
            predicate: Arc::new(|n| n == 2),
            timeout: Duration::from_secs(4),
            wait_until_timeout_elapses: false,
        };

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count_with_timeout("system ready", false, options)
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(2)).await;

        mock_provider.logs_push(vec!["system ready"]);

        let log_line_count = task.await?;

        assert!(matches!(log_line_count, LogLineCount::TargetReached(2)));

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_target_failed_timeout() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            "stub line 2",
            "system ready",
        ]);

        // Wait (up to 2 seconds) until pattern occurs twice
        let options = LogLineCountOptions {
            predicate: Arc::new(|n| n == 2),
            timeout: Duration::from_secs(2),
            wait_until_timeout_elapses: false,
        };

        let log_line_count = mock_node
            .wait_log_line_count_with_timeout("system ready", false, options)
            .await?;

        assert!(matches!(log_line_count, LogLineCount::TargetFailed(1)));

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_target_failed_exceeded() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            "stub line 2",
            "system ready",
        ]);

        // Wait until timeout and check if pattern occurs exactly twice
        let options = LogLineCountOptions {
            predicate: Arc::new(|n| n == 2),
            timeout: Duration::from_secs(2),
            wait_until_timeout_elapses: true,
        };

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count_with_timeout("system ready", false, options)
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec!["system ready"]);
        mock_provider.logs_push(vec!["system ready"]);

        let log_line_count = task.await?;

        assert!(matches!(log_line_count, LogLineCount::TargetFailed(3)));

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_target_reached_no_occurences() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec!["system booting", "stub line 1", "stub line 2"]);

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count_with_timeout(
                        "system ready",
                        false,
                        // Wait until timeout and make sure pattern occurred zero times
                        LogLineCountOptions::no_occurences_within_timeout(Duration::from_secs(2)),
                    )
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec!["stub line 3"]);

        assert!(task.await?.success());

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_target_reached_in_range() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec!["system booting", "stub line 1", "stub line 2"]);

        // Wait until timeout and make sure pattern occurrence count is in range between 2 and 5
        let options = LogLineCountOptions {
            predicate: Arc::new(|n| (2..=5).contains(&n)),
            timeout: Duration::from_secs(2),
            wait_until_timeout_elapses: true,
        };

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count_with_timeout("system ready", false, options)
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec!["system ready", "system ready", "system ready"]);

        assert!(task.await?.success());

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_with_timeout_with_lookahead_regex() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            // this line should not match
            "Error importing block 0xfd66e545c446b1c01205503130b816af0ec2c0e504a8472808e6ff4a644ce1fa: block has an unknown parent",
            "stub line 2"
        ]);

        let options = LogLineCountOptions {
            predicate: Arc::new(|n| n == 1),
            timeout: Duration::from_secs(3),
            wait_until_timeout_elapses: true,
        };

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count_with_timeout(
                        "error(?! importing block .*: block has an unknown parent)",
                        false,
                        options,
                    )
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec![
            "system ready",
            // this line should match
            "system error",
            "system ready",
        ]);

        assert!(task.await?.success());

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_with_timeout_with_lookahead_regex_fails(
    ) -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            // this line should not match
            "Error importing block 0xfd66e545c446b1c01205503130b816af0ec2c0e504a8472808e6ff4a644ce1fa: block has an unknown parent",
            "stub line 2"
        ]);

        let options = LogLineCountOptions {
            predicate: Arc::new(|n| n == 1),
            timeout: Duration::from_secs(6),
            wait_until_timeout_elapses: true,
        };

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count_with_timeout(
                        "error(?! importing block .*: block has an unknown parent)",
                        false,
                        options,
                    )
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec!["system ready", "system ready"]);

        assert!(!task.await?.success());

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_with_lockahead_regex() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            // this line should not match
            "Error importing block 0xfd66e545c446b1c01205503130b816af0ec2c0e504a8472808e6ff4a644ce1fa: block has an unknown parent",
            "stub line 2"
        ]);

        let task = tokio::spawn({
            async move {
                mock_node
                    .wait_log_line_count(
                        "error(?! importing block .*: block has an unknown parent)",
                        false,
                        1,
                    )
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec![
            "system ready",
            // this line should match
            "system error",
            "system ready",
        ]);

        assert!(task.await.is_ok());

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wait_log_count_with_lookahead_regex_fails() -> Result<(), anyhow::Error> {
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "node1",
            "ws_uri",
            "prometheus_uri",
            "multiaddr",
            NodeSpec::default(),
            mock_provider.clone(),
        );

        mock_provider.logs_push(vec![
            "system booting",
            "stub line 1",
            // this line should not match
            "Error importing block 0xfd66e545c446b1c01205503130b816af0ec2c0e504a8472808e6ff4a644ce1fa: block has an unknown parent",
            "stub line 2"
        ]);

        let options = LogLineCountOptions {
            predicate: Arc::new(|count| count == 1),
            timeout: Duration::from_secs(2),
            wait_until_timeout_elapses: true,
        };

        let task = tokio::spawn({
            async move {
                // we expect no match, thus wait with timeout
                mock_node
                    .wait_log_line_count_with_timeout(
                        "error(?! importing block .*: block has an unknown parent)",
                        false,
                        options,
                    )
                    .await
                    .unwrap()
            }
        });

        tokio::time::sleep(Duration::from_secs(1)).await;

        mock_provider.logs_push(vec!["system ready", "system ready"]);

        assert!(!task.await?.success());

        Ok(())
    }
}
