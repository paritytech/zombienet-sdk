use std::time::Duration;

use serde_json::Value;
use tokio::time::timeout;
use zombienet_sdk::NetworkNode;

use super::{
    evidence,
    live::{lookup_node, open_network},
};
use crate::{
    input::MetricInput,
    report::{Category, DiagnosticReport, Severity},
};

const MAX_METRICS_BYTES: usize = 1024 * 1024; // 1 MiB

pub async fn query_metric(input: MetricInput) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Metric query completed");

    if !evidence::validate_input(
        &mut report,
        input.validate(),
        input.node_name.clone(),
        &input.zombie_json_path,
        "Metric input is invalid",
    ) {
        evidence::finalize(&mut report);
        return report;
    }

    let Some(network) = open_network(&mut report, &input.zombie_json_path).await else {
        evidence::finalize(&mut report);
        return report;
    };

    let Some(node) = lookup_node(
        &mut report,
        &network,
        &input.node_name,
        &input.zombie_json_path,
    ) else {
        evidence::finalize(&mut report);
        return report;
    };

    let prometheus_uri = match node_prometheus_uri(node) {
        Ok(uri) => uri,
        Err(error) => {
            evidence::push(
                &mut report,
                Severity::Warning,
                format!(
                    "node.{}.metric.{}_endpoint_failed",
                    input.node_name, input.metric_name
                ),
                Category::Metrics,
                input.node_name.clone(),
                "Prometheus endpoint could not be resolved",
                input.zombie_json_path.display().to_string(),
                Some(error.to_string()),
            );
            evidence::finalize(&mut report);
            return report;
        },
    };

    let result = timeout(
        Duration::from_secs(input.timeout_secs),
        fetch_metric_value(&prometheus_uri, &input.metric_name),
    )
    .await;

    let metric_id = format!("node.{}.metric.{}", input.node_name, input.metric_name);
    match result {
        Ok(Ok(value)) => evidence::push(
            &mut report,
            Severity::Info,
            &metric_id,
            Category::Metrics,
            input.node_name.clone(),
            format!("Metric {} reported {}", input.metric_name, value),
            prometheus_uri,
            None,
        ),
        Ok(Err(error)) => evidence::push(
            &mut report,
            Severity::Warning,
            format!("{metric_id}_failed"),
            Category::Metrics,
            input.node_name.clone(),
            "Metric could not be queried",
            prometheus_uri,
            Some(error.to_string()),
        ),
        Err(_) => evidence::push(
            &mut report,
            Severity::Warning,
            format!("{metric_id}_timeout"),
            Category::Metrics,
            input.node_name.clone(),
            "Timed out querying metric",
            prometheus_uri,
            Some(format!("timeout_secs={}", input.timeout_secs)),
        ),
    }

    evidence::finalize(&mut report);
    report
}

pub(super) async fn fetch_metric_value(
    prometheus_uri: &str,
    metric_name: &str,
) -> Result<f64, anyhow::Error> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .get(prometheus_uri)
        .send()
        .await?
        .error_for_status()?;

    if response.status().is_redirection() {
        return Err(anyhow::anyhow!(
            "prometheus response redirected and will not be followed: status={}",
            response.status(),
        ));
    }

    if let Some(content_length) = response.content_length() {
        if content_length > MAX_METRICS_BYTES as u64 {
            return Err(anyhow::anyhow!(
                "prometheus response exceeded byte limit: content_length={}, max_bytes={}",
                content_length,
                MAX_METRICS_BYTES,
            ));
        }
    }

    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let next_len = body.len().saturating_add(chunk.len());
        if next_len > MAX_METRICS_BYTES {
            return Err(anyhow::anyhow!(
                "prometheus response exceeded byte limit: bytes={}, max_bytes={}",
                next_len,
                MAX_METRICS_BYTES,
            ));
        }
        body.extend_from_slice(&chunk);
    }

    parse_metric_value(&String::from_utf8_lossy(&body), metric_name)
}

/// Read the node's Prometheus endpoint URL.
// TODO: zombienet-sdk does not expose a `prometheus_uri()` getter, so we round-trip
// the node through serde to read the field. Replace with a public accessor once it lands.
pub(super) fn node_prometheus_uri(node: &NetworkNode) -> Result<String, anyhow::Error> {
    let value = serde_json::to_value(node)?;
    value
        .get("prometheus_uri")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("node prometheus_uri was not present"))
}

pub(super) fn parse_metric_value(
    metrics_raw: &str,
    metric_name: &str,
) -> Result<f64, anyhow::Error> {
    let metrics = prom_metrics_parser::parse(metrics_raw)?;
    metrics
        .get(metric_name)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("MetricNotFound: {metric_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{diagnostics::test_helpers::temp_zombie_json, report::Status};

    #[tokio::test]
    async fn fetch_metric_value_rejects_oversized_stream_without_content_length() {
        use tokio::{io::AsyncWriteExt, net::TcpListener, time::Duration};

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server can bind");
        let address = listener.local_addr().expect("test server has local addr");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("client can connect");
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .await
                .expect("headers can be written");
            socket
                .write_all(&vec![b'a'; MAX_METRICS_BYTES + 1])
                .await
                .expect("body can be written");
            std::future::pending::<()>().await;
        });

        let error = timeout(
            Duration::from_secs(2),
            fetch_metric_value(
                &format!("http://{address}/metrics"),
                "process_start_time_seconds",
            ),
        )
        .await
        .expect("oversized metrics stream should fail before EOF")
        .expect_err("oversized metrics stream should be rejected");

        server.abort();

        let message = error.to_string();
        assert!(message.contains("prometheus response exceeded byte limit"));
        assert!(message.contains(&format!("max_bytes={MAX_METRICS_BYTES}")));
    }

    #[tokio::test]
    async fn fetch_metric_value_rejects_redirects() {
        use tokio::{io::AsyncWriteExt, net::TcpListener};

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server can bind");
        let address = listener.local_addr().expect("test server has local addr");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("client can connect");
            socket
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://example.com/metrics\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("redirect response can be written");
        });

        let error = fetch_metric_value(
            &format!("http://{address}/metrics"),
            "process_start_time_seconds",
        )
        .await
        .expect_err("prometheus redirects should be rejected");

        server.await.expect("test server can finish");

        assert!(error.to_string().contains("redirected"));
    }

    #[test]
    fn parse_metric_value_returns_zero_when_metric_reports_zero() {
        let value = parse_metric_value(
            "# HELP polkadot_node_roles Node roles\n# TYPE polkadot_node_roles gauge\npolkadot_node_roles 0\n",
            "polkadot_node_roles",
        )
        .expect("zero-valued metric should be returned");

        assert_eq!(value, 0.0);
    }

    #[test]
    fn parse_metric_value_rejects_absent_metric() {
        let error = parse_metric_value(
            "# HELP polkadot_node_roles Node roles\n# TYPE polkadot_node_roles gauge\npolkadot_node_roles 0\n",
            "polkadot_missing_metric",
        )
        .expect_err("absent metrics should not be reported as zero");

        assert!(error.to_string().contains("MetricNotFound"));
    }

    #[tokio::test]
    async fn query_metric_reports_native_attach_error_as_evidence() {
        let path = temp_zombie_json("metric-native-attach-error", r#"{"relay": {"nodes": []}}"#);

        let report = query_metric(MetricInput {
            zombie_json_path: path.path().clone(),
            node_name: "alice".to_string(),
            metric_name: "process_start_time_seconds".to_string(),
            timeout_secs: 10,
        })
        .await;

        assert_eq!(report.status, Status::Failed);
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "network.attach_failed"
                && e.severity == Severity::Error
                && e.source == path.path().display().to_string()));
    }
}
