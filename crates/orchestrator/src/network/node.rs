use std::{sync::Arc, time::Duration};

use anyhow::anyhow;
use glob_match::glob_match;
use prom_metrics_parser::MetricMap;
use provider::DynNode;
use regex::Regex;
use serde::Serialize;
use subxt::{backend::rpc::RpcClient, OnlineClient};
use support::net::wait_ws_ready;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::network_spec::node::NodeSpec;
#[cfg(feature = "pjs")]
use crate::pjs_helper::{pjs_build_template, pjs_exec, PjsResult, ReturnValue};

#[derive(Error, Debug)]
pub enum NetworkNodeError {
    #[error("metric '{0}' not found!")]
    MetricNotFound(String),
}

#[derive(Clone, Serialize)]
pub struct NetworkNode {
    #[serde(skip)]
    pub(crate) inner: DynNode,
    // TODO: do we need the full spec here?
    // Maybe a reduce set of values.
    pub(crate) spec: NodeSpec,
    pub(crate) name: String,
    pub(crate) ws_uri: String,
    pub(crate) prometheus_uri: String,
    #[serde(skip)]
    metrics_cache: Arc<RwLock<MetricMap>>,
}

impl NetworkNode {
    /// Create a new NetworkNode
    pub(crate) fn new<T: Into<String>>(
        name: T,
        ws_uri: T,
        prometheus_uri: T,
        spec: NodeSpec,
        inner: DynNode,
    ) -> Self {
        Self {
            name: name.into(),
            ws_uri: ws_uri.into(),
            prometheus_uri: prometheus_uri.into(),
            inner,
            spec,
            metrics_cache: Arc::new(Default::default()),
        }
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

    // Subxt

    /// Get the rpc client for the node
    pub async fn rpc(&self) -> Result<RpcClient, subxt::Error> {
        RpcClient::from_url(&self.ws_uri).await
    }

    /// Get the [online client](subxt::client::OnlineClient) for the node
    pub async fn client<Config: subxt::Config>(
        &self,
    ) -> Result<OnlineClient<Config>, subxt::Error> {
        if subxt::utils::url_is_secure(&self.ws_uri)? {
            OnlineClient::from_url(&self.ws_uri).await
        } else {
            OnlineClient::from_insecure_url(&self.ws_uri).await
        }
    }

    /// Wait until get the [online client](subxt::client::OnlineClient) for the node
    pub async fn wait_client<Config: subxt::Config>(
        &self,
    ) -> Result<OnlineClient<Config>, anyhow::Error> {
        wait_ws_ready(self.ws_uri())
            .await
            .map_err(|e| anyhow!("Error awaiting http_client to ws be ready, err: {}", e))?;

        self.client()
            .await
            .map_err(|e| anyhow!("Can't create a subxt client, err: {}", e))
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
        self.inner.pause().await?;
        Ok(())
    }

    /// Resume the node, this is implemented by resuming the
    /// actual process (e.g polkadot) with sending `SIGCONT` signal
    pub async fn resume(&self) -> Result<(), anyhow::Error> {
        self.inner.resume().await?;
        Ok(())
    }

    /// Restart the node using the same `cmd`, `args` and `env` (and same isolated dir)
    pub async fn restart(&self, after: Option<Duration>) -> Result<(), anyhow::Error> {
        self.inner.restart(after).await?;
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
        self.metric(&metric_name).await
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
    /// See [`reports`] description for details on metric name.
    pub async fn assert_with(
        &self,
        metric_name: impl Into<String>,
        predicate: impl Fn(f64) -> bool,
    ) -> Result<bool, anyhow::Error> {
        let metric_name = metric_name.into();
        let val = self.metric(&metric_name).await?;
        if predicate(val) {
            Ok(true)
        } else {
            // reload metrics
            self.fetch_metrics().await?;
            let val = self.metric(&metric_name).await?;
            trace!("🔎 Current value passed to the predicated: {val}");
            Ok(predicate(val))
        }
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
                Err(e) => {
                    match e.downcast::<reqwest::Error>() {
                        Ok(io) => {
                            // if the error is connecting could be the case that the node
                            // is not listening yet, so we keep waiting
                            // Skipped err is: 'tcp connect error: Connection refused (os error 61)'
                            if !io.is_connect() {
                                return Err(io.into());
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
                    }
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
                Err(e) => Err(anyhow!("Error waiting for metric: {}", e)),
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
    pub async fn wait_log_line_count<'a>(
        &self,
        pattern: impl Into<String>,
        is_glob: bool,
        count: usize,
    ) -> Result<(), anyhow::Error> {
        let pattern: String = pattern.into();
        debug!("waiting until we find pattern {pattern} {count} times");
        let match_fn: Box<dyn Fn(&str) -> bool> = if is_glob {
            Box::new(|line: &str| -> bool { glob_match(&pattern, line) })
        } else {
            let re = Regex::new(&pattern)?;
            Box::new(move |line: &str| -> bool { re.is_match(line) })
        };

        loop {
            let mut q = 0_usize;
            let logs = self.logs().await?;
            for line in logs.lines() {
                trace!("line is {line}");
                if match_fn(line) {
                    trace!("pattern {pattern} match in line {line}");
                    q += 1;
                    if q >= count {
                        return Ok(());
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Wait until a the number of matching log lines is reach
    /// with timeout (secs)
    pub async fn wait_log_line_count_with_timeout(
        &self,
        substring: impl Into<String>,
        is_glob: bool,
        count: usize,
        timeout_secs: impl Into<u64>,
    ) -> Result<(), anyhow::Error> {
        let secs = timeout_secs.into();
        debug!("waiting until match {count} lines");
        tokio::time::timeout(
            Duration::from_secs(secs),
            self.wait_log_line_count(substring, is_glob, count),
        )
        .await?
    }

    // TODO: impl
    // wait_event_count
    // wait_event_count_with_timeout

    #[cfg(feature = "pjs")]
    /// Execute js/ts code inside [pjs_rs] custom runtime.
    ///
    /// The code will be run in a wrapper similar to the `javascript` developer tab
    /// of polkadot.js apps. The returning value is represented as [PjsResult] enum, to allow
    /// to communicate that the execution was successful but the returning value can be deserialized as [serde_json::Value].
    pub async fn pjs(
        &self,
        code: impl AsRef<str>,
        args: Vec<serde_json::Value>,
        user_types: Option<serde_json::Value>,
    ) -> Result<PjsResult, anyhow::Error> {
        let code = pjs_build_template(self.ws_uri(), code.as_ref(), args, user_types);
        tracing::trace!("Code to execute: {code}");
        let value = match pjs_exec(code)? {
            ReturnValue::Deserialized(val) => Ok(val),
            ReturnValue::CantDeserialize(msg) => Err(msg),
        };

        Ok(value)
    }

    #[cfg(feature = "pjs")]
    /// Execute js/ts file  inside [pjs_rs] custom runtime.
    ///
    /// The content of the file will be run in a wrapper similar to the `javascript` developer tab
    /// of polkadot.js apps. The returning value is represented as [PjsResult] enum, to allow
    /// to communicate that the execution was successful but the returning value can be deserialized as [serde_json::Value].
    pub async fn pjs_file(
        &self,
        file: impl AsRef<std::path::Path>,
        args: Vec<serde_json::Value>,
        user_types: Option<serde_json::Value>,
    ) -> Result<PjsResult, anyhow::Error> {
        let content = std::fs::read_to_string(file)?;
        self.pjs(content, args, user_types).await
    }

    async fn fetch_metrics(&self) -> Result<(), anyhow::Error> {
        let response = reqwest::get(&self.prometheus_uri).await?;
        let metrics = prom_metrics_parser::parse(&response.text().await?)?;
        let mut cache = self.metrics_cache.write().await;
        *cache = metrics;
        Ok(())
    }

    async fn metric(
        &self,
        metric_name: &str, // treat_not_found_as_zero: bool
    ) -> Result<f64, anyhow::Error> {
        // TODO: allow to pass as arg
        let treat_not_found_as_zero = true;
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

// TODO: mock and impl unit tests
