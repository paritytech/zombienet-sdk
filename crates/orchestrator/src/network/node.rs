use std::{
    collections::HashMap,
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
use serde::{Deserialize, Serialize, Serializer};
use subxt::{backend::rpc::RpcClient, OnlineClient};
use support::net::{skip_err_while_waiting, wait_ws_ready};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};

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

#[derive(Deserialize)]
pub(crate) struct RawNetworkNode {
    pub(crate) name: String,
    pub(crate) ws_uri: String,
    pub(crate) prometheus_uri: String,
    pub(crate) multiaddr: String,
    pub(crate) spec: NodeSpec,
    pub(crate) inner: serde_json::Value,
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
    ///
    /// Note: If you're using this method with the native provider on the attached network, the live network has to be running
    /// with global setting `teardown_on_failure` disabled.
    pub async fn pause(&self) -> Result<(), anyhow::Error> {
        self.set_is_running(false);
        self.inner.pause().await?;
        Ok(())
    }

    /// Resume the node, this is implemented by resuming the
    /// actual process (e.g polkadot) with sending `SIGCONT` signal
    ///
    /// Note: If you're using this method with the native provider on the attached network, the live network has to be running
    /// with global setting `teardown_on_failure` disabled.
    pub async fn resume(&self) -> Result<(), anyhow::Error> {
        self.set_is_running(true);
        self.inner.resume().await?;
        Ok(())
    }

    /// Restart the node using the same `cmd`, `args` and `env` (and same isolated dir)
    ///
    /// Note: If you're using this method with the native provider on the attached network, the live network has to be running
    /// with global setting `teardown_on_failure` disabled.
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

    /// Fetches histogram buckets for a given metric from the Prometheus endpoint.
    ///
    /// This function retrieves histogram bucket data by parsing the Prometheus metrics
    /// and calculating the count of observations in each bucket. It automatically appends
    /// `_bucket` suffix to the metric name if not already present.
    ///
    /// # Arguments
    /// * `metric_name` - The name of the histogram metric (with or without `_bucket` suffix)
    /// * `label_filters` - Optional HashMap of label key-value pairs to filter metrics by
    ///
    /// # Returns
    /// A HashMap where keys are the `le` bucket boundaries as strings,
    /// and values are the count of observations in each bucket (calculated as delta from previous bucket).
    ///
    /// # Example
    /// ```ignore
    /// let buckets = node.get_histogram_buckets("polkadot_pvf_execution_time", None).await?;
    /// // Returns: {"0.1": 5, "0.5": 10, "1.0": 3, "+Inf": 0}
    /// ```
    pub async fn get_histogram_buckets(
        &self,
        metric_name: impl AsRef<str>,
        label_filters: Option<HashMap<String, String>>,
    ) -> Result<HashMap<String, u64>, anyhow::Error> {
        let metric_name = metric_name.as_ref();

        // Fetch raw metrics text
        let response = reqwest::get(&self.prometheus_uri).await?;
        let metrics_text = response.text().await?;

        // Ensure metric name has _bucket suffix
        let resolved_metric_name = if metric_name.contains("_bucket") {
            metric_name.to_string()
        } else {
            format!("{}_bucket", metric_name)
        };

        let mut raw_buckets: Vec<(String, u64)> = Vec::new();
        let mut active_series_labels: Option<HashMap<String, String>> = None;

        // Parse metrics line by line
        for line in metrics_text.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse the line manually to extract metric name, labels, and value
            if let Some((name_with_labels, value_str)) = line.split_once(char::is_whitespace) {
                let name_with_labels = name_with_labels.trim();

                // Check if this line matches our metric name
                let (name, labels_str) = if let Some(idx) = name_with_labels.find('{') {
                    (&name_with_labels[..idx], &name_with_labels[idx..])
                } else {
                    (name_with_labels, "")
                };

                if name != resolved_metric_name {
                    continue;
                }

                // Parse labels if present
                let mut parsed_labels: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                let mut le_label: Option<String> = None;

                if !labels_str.is_empty()
                    && labels_str.starts_with('{')
                    && labels_str.ends_with('}')
                {
                    let labels_content = &labels_str[1..labels_str.len() - 1];
                    parsed_labels = Self::parse_prometheus_labels(labels_content);
                    le_label = parsed_labels.get("le").cloned();
                }

                // Check if all required label filters match
                if let Some(ref filters) = label_filters {
                    let mut all_match = true;
                    for (filter_key, filter_value) in filters {
                        if parsed_labels.get(filter_key) != Some(filter_value) {
                            all_match = false;
                            break;
                        }
                    }

                    if !all_match {
                        continue;
                    }
                }

                // Get non-"le" labels to detect series changes
                let mut series_labels = parsed_labels.clone();
                series_labels.remove("le");

                // Check if we're starting a new series (different non-le labels)
                if let Some(ref prev_labels) = active_series_labels {
                    if prev_labels != &series_labels {
                        // New series detected, stop processing if we already have data
                        if !raw_buckets.is_empty() {
                            break;
                        }
                        // Otherwise reset for new series
                        active_series_labels = Some(series_labels);
                    }
                } else {
                    // First series
                    active_series_labels = Some(series_labels);
                }

                // Extract and collect the metric value
                if let Some(le) = le_label {
                    if let Ok(metric_value) = value_str.trim().parse::<u64>() {
                        trace!("{} le:{} {}", resolved_metric_name, &le, metric_value);
                        raw_buckets.push((le, metric_value));
                    }
                }
            }
        }

        raw_buckets.sort_by(|a, b| Self::compare_le_values(&a.0, &b.0));

        let mut buckets = HashMap::new();
        let mut previous_value = 0_u64;
        for (le, cumulative_count) in raw_buckets {
            if cumulative_count < previous_value {
                warn!(
                    "Warning: bucket count decreased from {} to {} at le={}",
                    previous_value, cumulative_count, le
                );
            }
            let delta = cumulative_count.saturating_sub(previous_value);
            buckets.insert(le, delta);
            previous_value = cumulative_count;
        }

        Ok(buckets)
    }

    /// Parse Prometheus label string handling quoted values with commas and escapes.
    ///
    /// Implements a simple state machine to correctly parse label key-value pairs
    /// that may contain commas within quoted values.
    fn parse_prometheus_labels(labels_str: &str) -> HashMap<String, String> {
        let mut labels = HashMap::new();
        let mut current_key = String::new();
        let mut current_value = String::new();
        let mut in_value = false;
        let mut in_quotes = false;
        let mut escape_next = false;

        for ch in labels_str.chars() {
            if escape_next {
                current_value.push(ch);
                escape_next = false;
                continue;
            }

            match ch {
                '\\' if in_quotes => {
                    escape_next = true;
                },
                '=' if !in_quotes && !in_value => {
                    in_value = true;
                },
                '"' if in_value => {
                    in_quotes = !in_quotes;
                },
                ',' if !in_quotes => {
                    // End of key-value pair
                    if !current_key.is_empty() {
                        labels.insert(
                            current_key.trim().to_string(),
                            current_value.trim().to_string(),
                        );
                        current_key.clear();
                        current_value.clear();
                        in_value = false;
                    }
                },
                _ => {
                    if in_value {
                        current_value.push(ch);
                    } else {
                        current_key.push(ch);
                    }
                },
            }
        }

        // Insert last pair
        if !current_key.is_empty() {
            labels.insert(
                current_key.trim().to_string(),
                current_value.trim().to_string(),
            );
        }

        labels
    }

    /// Compare two histogram bucket boundary values for sorting.
    ///
    /// Treats "+Inf" as the maximum value, otherwise compares numerically.
    fn compare_le_values(a: &str, b: &str) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        // Handle +Inf specially
        match (a, b) {
            ("+Inf", "+Inf") => Ordering::Equal,
            ("+Inf", _) => Ordering::Greater,
            (_, "+Inf") => Ordering::Less,
            _ => {
                // Try to parse as f64 for numeric comparison
                match (a.parse::<f64>(), b.parse::<f64>()) {
                    (Ok(a_val), Ok(b_val)) => a_val.partial_cmp(&b_val).unwrap_or(Ordering::Equal),
                    // Fallback to string comparison if parsing fails
                    _ => a.cmp(b),
                }
            },
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

    #[tokio::test]
    async fn test_get_histogram_buckets_parsing() -> Result<(), anyhow::Error> {
        // This test uses a mock HTTP server to simulate Prometheus metrics
        use std::sync::Arc;

        // Create a mock metrics response
        let mock_metrics = r#"
                # HELP substrate_block_verification_time Time taken to verify blocks
                # TYPE substrate_block_verification_time histogram
                substrate_block_verification_time_bucket{chain="rococo_local_testnet",le="0.1"} 10
                substrate_block_verification_time_bucket{chain="rococo_local_testnet",le="0.5"} 25
                substrate_block_verification_time_bucket{chain="rococo_local_testnet",le="1.0"} 35
                substrate_block_verification_time_bucket{chain="rococo_local_testnet",le="2.5"} 40
                substrate_block_verification_time_bucket{chain="rococo_local_testnet",le="+Inf"} 42
                substrate_block_verification_time_sum{chain="rococo_local_testnet"} 45.5
                substrate_block_verification_time_count{chain="rococo_local_testnet"} 42
                # Different chain
                substrate_block_verification_time_bucket{chain="kusama",le="0.1"} 5
                substrate_block_verification_time_bucket{chain="kusama",le="0.5"} 15
                "#;

        // Start a mock HTTP server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let metrics = Arc::new(mock_metrics.to_string());

        tokio::spawn({
            let metrics = metrics.clone();
            async move {
                loop {
                    if let Ok((mut socket, _)) = listener.accept().await {
                        let metrics = metrics.clone();
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buffer = [0; 1024];
                            let _ = socket.read(&mut buffer).await;

                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                                metrics.len(),
                                metrics
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                        });
                    }
                }
            }
        });

        // Create a NetworkNode with the mock prometheus URI
        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "test_node",
            "ws://localhost:9944",
            &format!("http://127.0.0.1:{}/metrics", addr.port()),
            "/ip4/127.0.0.1/tcp/30333",
            NodeSpec::default(),
            mock_provider,
        );

        // Get buckets without label filter
        let buckets = mock_node
            .get_histogram_buckets("substrate_block_verification_time", None)
            .await?;

        // Should get the first chain's buckets (rococo_local_testnet)
        assert_eq!(buckets.get("0.1"), Some(&10));
        assert_eq!(buckets.get("0.5"), Some(&15)); // 25 - 10
        assert_eq!(buckets.get("1.0"), Some(&10)); // 35 - 25
        assert_eq!(buckets.get("2.5"), Some(&5)); // 40 - 35
        assert_eq!(buckets.get("+Inf"), Some(&2)); // 42 - 40

        // Get buckets with label filter for rococo
        let mut label_filters = std::collections::HashMap::new();
        label_filters.insert("chain".to_string(), "rococo_local_testnet".to_string());

        let buckets_filtered = mock_node
            .get_histogram_buckets("substrate_block_verification_time", Some(label_filters))
            .await?;

        assert_eq!(buckets_filtered.get("0.1"), Some(&10));
        assert_eq!(buckets_filtered.get("0.5"), Some(&15));

        // Test 3: Get buckets with _bucket suffix already present
        let buckets_with_suffix = mock_node
            .get_histogram_buckets("substrate_block_verification_time_bucket", None)
            .await?;

        assert_eq!(buckets_with_suffix.get("0.1"), Some(&10));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_histogram_buckets_unordered() -> Result<(), anyhow::Error> {
        // Test that buckets are correctly sorted even when received out of order
        use std::sync::Arc;

        let mock_metrics = r#"
                test_metric_bucket{le="2.5"} 40
                test_metric_bucket{le="0.1"} 10
                test_metric_bucket{le="+Inf"} 42
                test_metric_bucket{le="1.0"} 35
                test_metric_bucket{le="0.5"} 25
                "#;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let metrics = Arc::new(mock_metrics.to_string());

        tokio::spawn({
            let metrics = metrics.clone();
            async move {
                loop {
                    if let Ok((mut socket, _)) = listener.accept().await {
                        let metrics = metrics.clone();
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buffer = [0; 1024];
                            let _ = socket.read(&mut buffer).await;
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                                metrics.len(),
                                metrics
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                        });
                    }
                }
            }
        });

        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "test_node",
            "ws://localhost:9944",
            &format!("http://127.0.0.1:{}/metrics", addr.port()),
            "/ip4/127.0.0.1/tcp/30333",
            NodeSpec::default(),
            mock_provider,
        );

        let buckets = mock_node.get_histogram_buckets("test_metric", None).await?;

        // Verify deltas are calculated correctly after sorting
        assert_eq!(buckets.get("0.1"), Some(&10)); // 10 - 0
        assert_eq!(buckets.get("0.5"), Some(&15)); // 25 - 10
        assert_eq!(buckets.get("1.0"), Some(&10)); // 35 - 25
        assert_eq!(buckets.get("2.5"), Some(&5)); // 40 - 35
        assert_eq!(buckets.get("+Inf"), Some(&2)); // 42 - 40

        Ok(())
    }

    #[tokio::test]
    async fn test_get_histogram_buckets_complex_labels() -> Result<(), anyhow::Error> {
        // Test label parsing with commas and special characters in values
        use std::sync::Arc;

        let mock_metrics = r#"
                test_metric_bucket{method="GET,POST",path="/api/test",le="0.1"} 5
                test_metric_bucket{method="GET,POST",path="/api/test",le="0.5"} 15
                test_metric_bucket{method="GET,POST",path="/api/test",le="+Inf"} 20
                "#;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let metrics = Arc::new(mock_metrics.to_string());

        tokio::spawn({
            let metrics = metrics.clone();
            async move {
                loop {
                    if let Ok((mut socket, _)) = listener.accept().await {
                        let metrics = metrics.clone();
                        tokio::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buffer = [0; 1024];
                            let _ = socket.read(&mut buffer).await;
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                                metrics.len(),
                                metrics
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                        });
                    }
                }
            }
        });

        let mock_provider = Arc::new(MockNode::new());
        let mock_node = NetworkNode::new(
            "test_node",
            "ws://localhost:9944",
            &format!("http://127.0.0.1:{}/metrics", addr.port()),
            "/ip4/127.0.0.1/tcp/30333",
            NodeSpec::default(),
            mock_provider,
        );

        // Test without filter
        let buckets = mock_node.get_histogram_buckets("test_metric", None).await?;
        assert_eq!(buckets.get("0.1"), Some(&5));
        assert_eq!(buckets.get("0.5"), Some(&10)); // 15 - 5
        assert_eq!(buckets.get("+Inf"), Some(&5)); // 20 - 15

        // Test with filter containing comma in value
        let mut label_filters = std::collections::HashMap::new();
        label_filters.insert("method".to_string(), "GET,POST".to_string());

        let buckets_filtered = mock_node
            .get_histogram_buckets("test_metric", Some(label_filters))
            .await?;

        assert_eq!(buckets_filtered.get("0.1"), Some(&5));
        assert_eq!(buckets_filtered.get("0.5"), Some(&10));

        Ok(())
    }

    #[test]
    fn test_parse_prometheus_labels() {
        use crate::network::node::NetworkNode;

        // Simple labels
        let labels = NetworkNode::parse_prometheus_labels(r#"le="0.1",chain="rococo""#);
        assert_eq!(labels.get("le"), Some(&"0.1".to_string()));
        assert_eq!(labels.get("chain"), Some(&"rococo".to_string()));

        // Labels with comma in value
        let labels = NetworkNode::parse_prometheus_labels(r#"method="GET,POST",path="/test""#);
        assert_eq!(labels.get("method"), Some(&"GET,POST".to_string()));
        assert_eq!(labels.get("path"), Some(&"/test".to_string()));

        // Labels with escaped quotes
        let labels = NetworkNode::parse_prometheus_labels(r#"name="test\"value\"",id="123""#);
        assert_eq!(labels.get("name"), Some(&"test\"value\"".to_string()));
        assert_eq!(labels.get("id"), Some(&"123".to_string()));

        // Empty labels
        let labels = NetworkNode::parse_prometheus_labels("");
        assert!(labels.is_empty());
    }

    #[test]
    fn test_compare_le_values() {
        use std::cmp::Ordering;

        use crate::network::node::NetworkNode;

        // Numeric comparison
        assert_eq!(NetworkNode::compare_le_values("0.1", "0.5"), Ordering::Less);
        assert_eq!(
            NetworkNode::compare_le_values("1.0", "0.5"),
            Ordering::Greater
        );
        assert_eq!(
            NetworkNode::compare_le_values("1.0", "1.0"),
            Ordering::Equal
        );

        // +Inf handling
        assert_eq!(
            NetworkNode::compare_le_values("+Inf", "999"),
            Ordering::Greater
        );
        assert_eq!(
            NetworkNode::compare_le_values("0.1", "+Inf"),
            Ordering::Less
        );
        assert_eq!(
            NetworkNode::compare_le_values("+Inf", "+Inf"),
            Ordering::Equal
        );

        // Large numbers
        assert_eq!(NetworkNode::compare_le_values("10", "100"), Ordering::Less);
        assert_eq!(
            NetworkNode::compare_le_values("1000", "999"),
            Ordering::Greater
        );
    }
}
