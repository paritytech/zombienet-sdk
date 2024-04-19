use std::{path::Path, sync::Arc, time::Duration};

use anyhow::anyhow;
use pjs_rs::ReturnValue;
use prom_metrics_parser::MetricMap;
use provider::DynNode;
use serde_json::json;
use subxt::{backend::rpc::RpcClient, OnlineClient};
use tokio::sync::RwLock;
use tracing::trace;

use crate::{network_spec::node::NodeSpec, shared::types::PjsResult};

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

    pub fn spec(&self) -> &NodeSpec {
        &self.spec
    }

    pub fn ws_uri(&self) -> &str {
        &self.ws_uri
    }

    /// Pause the node, this is implemented by pausing the
    /// actual process (e.g polkadot) with sending `SIGSTOP` signal
    pub async fn pause(&self) -> Result<(), anyhow::Error> {
        self.inner.pause().await?;
        Ok(())
    }

    /// Get the rpc client for the node
    pub async fn rpc(&self) -> Result<RpcClient, subxt::Error> {
        RpcClient::from_url(&self.ws_uri).await
    }

    /// Get the online client for the node
    pub async fn client<Config: subxt::Config>(
        &self,
    ) -> Result<OnlineClient<Config>, subxt::Error> {
        OnlineClient::from_url(&self.ws_uri).await
    }

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
        trace!("Code to execute: {code}");
        let value = match pjs_inner(code)? {
            ReturnValue::Deserialized(val) => Ok(val),
            ReturnValue::CantDeserialize(msg) => Err(msg),
        };

        Ok(value)
    }

    /// Execute js/ts file  inside [pjs_rs] custom runtime.
    ///
    /// The content of the file will be run in a wrapper similar to the `javascript` developer tab
    /// of polkadot.js apps. The returning value is represented as [PjsResult] enum, to allow
    /// to communicate that the execution was successful but the returning value can be deserialized as [serde_json::Value].
    pub async fn pjs_file(
        &self,
        file: impl AsRef<Path>,
        args: Vec<serde_json::Value>,
        user_types: Option<serde_json::Value>,
    ) -> Result<PjsResult, anyhow::Error> {
        let content = std::fs::read_to_string(file)?;
        let code = pjs_build_template(self.ws_uri(), content.as_ref(), args, user_types);
        trace!("Code to execute: {code}");

        let value = match pjs_inner(code)? {
            ReturnValue::Deserialized(val) => Ok(val),
            ReturnValue::CantDeserialize(msg) => Err(msg),
        };

        Ok(value)
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
        let value: f64 = value.into();
        self.assert_with(metric_name, |v| v == value).await
    }

    /// Assert on a metric value using a given predicate.
    /// See [`assert`] description for details.
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
            Ok(predicate(val))
        }
    }

    /// Get the logs of the node
    /// TODO: do we need the `since` param, maybe we could be handy later for loop filtering
    pub async fn logs(&self) -> Result<String, anyhow::Error> {
        Ok(self.inner.logs().await?)
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

// Helper methods

fn pjs_build_template(
    ws_uri: &str,
    content: &str,
    args: Vec<serde_json::Value>,
    user_types: Option<serde_json::Value>,
) -> String {
    let types = if let Some(user_types) = user_types {
        if let Some(types) = user_types.pointer("/types") {
            // if the user_types includes the `types` key use the inner value
            types.clone()
        } else {
            user_types.clone()
        }
    } else {
        // No custom types, just an emtpy json
        json!({})
    };

    let tmpl = format!(
        r#"
    const {{ util, utilCrypto, keyring, types }} = pjs;
    ( async () => {{
        const api = await pjs.api.ApiPromise.create({{
            provider: new pjs.api.WsProvider('{}'),
            types: {}
         }});
        const _run = async (api, hashing, keyring, types, util, arguments) => {{
            {}
        }};
        return await _run(api, utilCrypto, keyring, types, util, {});
    }})()
    "#,
        ws_uri,
        types,
        content,
        json!(args),
    );
    trace!(tmpl = tmpl, "code to execute");
    tmpl
}

// Since pjs-rs run a custom javascript runtime (using deno_core) we need to
// execute in an isolated thread.
fn pjs_inner(code: String) -> Result<ReturnValue, anyhow::Error> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    std::thread::spawn(move || {
        rt.block_on(async move {
            let value = pjs_rs::run_ts_code(code, None).await;
            trace!("ts_code return: {:?}", value);
            value
        })
    })
    .join()
    .map_err(|_| anyhow!("[pjs] Thread panicked"))?
}
