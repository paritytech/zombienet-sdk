use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Url;

#[async_trait]
pub trait MetricsHelper {
    async fn metric(&self, metric_name: &str) -> Result<f64, anyhow::Error>;
    async fn metric_with_url(
        metric: impl AsRef<str> + Send,
        endpoint: impl Into<Url> + Send,
    ) -> Result<f64, anyhow::Error>;
}

pub struct Metrics {
    endpoint: Url,
}

impl Metrics {
    fn new(endpoint: impl Into<Url>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub async fn fetch_metrics(
        endpoint: impl AsRef<str>,
    ) -> Result<HashMap<String, f64>, anyhow::Error> {
        let response = reqwest::get(endpoint.as_ref()).await?;
        Ok(prom_metrics_parser::parse(&response.text().await?)?)
    }

    pub fn get_metric(
        metrics_map: HashMap<String, f64>,
        metric_name: &str,
        treat_not_found_as_zero: bool,
    ) -> Result<f64, anyhow::Error> {
        if let Some(val) = metrics_map.get(metric_name) {
            Ok(*val)
        } else if treat_not_found_as_zero {
            Ok(0_f64)
        } else {
            Err(anyhow::anyhow!("MetricNotFound: {metric_name}"))
        }
    }
}

#[async_trait]
impl MetricsHelper for Metrics {
    async fn metric(&self, metric_name: &str) -> Result<f64, anyhow::Error> {
        let metrics_map = Metrics::fetch_metrics(self.endpoint.as_str()).await?;
        Metrics::get_metric(metrics_map, metric_name, true)
    }

    async fn metric_with_url(
        metric_name: impl AsRef<str> + Send,
        endpoint: impl Into<Url> + Send,
    ) -> Result<f64, anyhow::Error> {
        let metrics_map = Metrics::fetch_metrics(endpoint.into()).await?;
        Metrics::get_metric(metrics_map, metric_name.as_ref(), true)
    }
}
