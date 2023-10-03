use std::{sync::Arc, time::Duration};

use anyhow::anyhow;
use prom_metrics_parser::MetricMap;
use provider::DynNode;
use tokio::sync::RwLock;

use crate::network_spec::node::NodeSpec;

#[derive(Clone)]
pub struct NetworkNode {
    pub(crate) inner: DynNode,
    // TODO: do we need the full spec here?
    // Maybe a reduce set of values.
    pub(crate) spec: NodeSpec,
    pub(crate) name: String,
    pub(crate) ws_uri: String,
    pub(crate) prometheus_uri: String,
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

    /// Pause the node, this is implemented by pausing the
    /// actual process (e.g polkadot) with sendig `SIGSTOP` signal
    pub async fn pause(&self) -> Result<(), anyhow::Error> {
        self.inner.pause().await?;
        Ok(())
    }

    /// Resume the node, this is implemented by resuming the
    /// actual process (e.g polkadot) with sendig `SIGCONT` signal
    pub async fn resume(&self) -> Result<(), anyhow::Error> {
        self.inner.resume().await?;
        Ok(())
    }

    /// Restart the node using the same `cmd`, `args` and `env` (and same isolated dir)
    pub async fn restart(&self, after: Option<Duration>) -> Result<(), anyhow::Error> {
        self.inner.restart(after).await?;
        Ok(())
    }

    /// Get metric value 'by name' from prometheus (exposed by the node)
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

    /// Assert on a metric value 'by name' from prometheus (exposed by the node)
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
        let metric_name = metric_name.into();
        let value = value.into();
        let val = self.metric(&metric_name).await?;
        if val == value {
            Ok(true)
        } else {
            // reload metrcis
            self.fetch_metrics().await?;
            let val = self.metric(&metric_name).await?;
            Ok(val == value)
        }
    }

    async fn fetch_metrics(&self) -> Result<(), anyhow::Error> {
        let response = reqwest::get(&self.prometheus_uri).await?;
        let metrics = prom_metrics_parser::parse(&response.text().await?)?;
        let mut cache = self.metrics_cache.write().await;
        *cache = metrics;
        Ok(())
    }

    async fn metric(&self, metric_name: &str) -> Result<f64, anyhow::Error> {
        let mut metrics_map = self.metrics_cache.read().await;
        if metrics_map.is_empty() {
            // reload metrics
            drop(metrics_map);
            self.fetch_metrics().await?;
            metrics_map = self.metrics_cache.read().await;
        }

        let val = metrics_map
            .get(metric_name)
            .ok_or(anyhow!("metric '{}'not found!", &metric_name))?;
        Ok(*val)
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
