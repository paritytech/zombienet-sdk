use std::time::Duration;

use tokio::time::timeout;
use zombienet_sdk::{subxt, NetworkNode};

use super::{
    evidence,
    live::{lookup_node, open_network},
    metrics::{fetch_metric_value, node_prometheus_uri},
    startup::MAX_LOG_BYTES,
};
use crate::{
    input::{BlockProductionInput, ListNodesInput, NodeInput, NodeLogsInput},
    log_patterns::scan_logs,
    report::{
        bounded_tail, bounded_tail_bytes, status_from_evidence, Category, DiagnosticReport,
        Severity,
    },
};

const EXCERPT_MAX_BYTES: usize = 8 * 1024;
const NODE_LOGS_TIMEOUT: Duration = Duration::from_secs(10);
const NODE_LIVENESS_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn list_nodes(input: ListNodesInput) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Node listing completed");

    let Some(network) = open_network(&mut report, &input.zombie_json_path).await else {
        evidence::finalize(&mut report);
        return report;
    };

    let nodes = network.nodes();
    for node in &nodes {
        evidence::push(
            &mut report,
            Severity::Info,
            format!("node.{}.listed", node.name()),
            Category::Liveness,
            node.name().to_string(),
            format!(
                "Node {} is present with websocket {} and multiaddr {}",
                node.name(),
                node.ws_uri(),
                node.multiaddr()
            ),
            input.zombie_json_path.display().to_string(),
            None,
        );
    }

    report.summary = format!("Found {} nodes", nodes.len());
    report.status = status_from_evidence(&report.evidence);
    report
}

pub async fn get_node_logs(input: NodeLogsInput) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Node log retrieval completed");

    if !evidence::validate_input(
        &mut report,
        input.validate(),
        input.node_name.clone(),
        &input.zombie_json_path,
        "Node log input is invalid",
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

    let node_report = collect_node_logs_for_node(
        node,
        input.zombie_json_path.display().to_string(),
        input.lines,
    )
    .await;
    report.evidence.extend(node_report.evidence);

    evidence::finalize(&mut report);
    report
}

pub async fn check_node_liveness(input: NodeInput) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Node liveness check completed");

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

    let node_report =
        check_node_liveness_for_node(node, input.zombie_json_path.display().to_string()).await;
    report.evidence.extend(node_report.evidence);

    evidence::finalize(&mut report);
    report
}

pub async fn check_block_production(input: BlockProductionInput) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Block production check completed");

    if !evidence::validate_input(
        &mut report,
        input.validate(),
        input.node_name.clone(),
        &input.zombie_json_path,
        "Block production input is invalid",
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

    let observed = timeout(
        Duration::from_secs(input.timeout_secs),
        collect_finalized_blocks(node, input.blocks),
    )
    .await;

    let id_prefix = format!("node.{}.blocks", input.node_name);
    let source = input.zombie_json_path.display().to_string();
    match observed {
        Ok(Ok(blocks)) => evidence::push(
            &mut report,
            Severity::Info,
            format!("{id_prefix}.finalized"),
            Category::Liveness,
            input.node_name.clone(),
            format!("Observed {} finalized blocks", blocks.len()),
            source,
            Some(blocks.join("\n")),
        ),
        Ok(Err(error)) => evidence::push(
            &mut report,
            Severity::Error,
            format!("{id_prefix}.failed"),
            Category::Liveness,
            input.node_name.clone(),
            "Could not observe finalized blocks",
            source,
            Some(error.to_string()),
        ),
        Err(_) => evidence::push(
            &mut report,
            Severity::Error,
            format!("{id_prefix}.timeout"),
            Category::Liveness,
            input.node_name.clone(),
            "Timed out observing finalized blocks",
            source,
            Some(format!(
                "expected_blocks={}, timeout_secs={}",
                input.blocks, input.timeout_secs
            )),
        ),
    }

    evidence::finalize(&mut report);
    report
}

pub(super) async fn check_node_liveness_for_node(
    node: &NetworkNode,
    source: String,
) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Node liveness check completed");
    let node_name = node.name().to_string();

    match timeout(NODE_LIVENESS_TIMEOUT, node.is_responsive()).await {
        Ok(true) => evidence::push(
            &mut report,
            Severity::Info,
            format!("node.{node_name}.rpc_responsive"),
            Category::Rpc,
            node_name.clone(),
            "Node RPC endpoint is responsive",
            source.clone(),
            Some(node.ws_uri().to_string()),
        ),
        Ok(false) => evidence::push(
            &mut report,
            Severity::Error,
            format!("node.{node_name}.rpc_unresponsive"),
            Category::Rpc,
            node_name.clone(),
            "Node RPC endpoint is not responsive",
            source.clone(),
            Some(node.ws_uri().to_string()),
        ),
        Err(_) => evidence::push(
            &mut report,
            Severity::Error,
            format!("node.{node_name}.rpc_timeout"),
            Category::Rpc,
            node_name.clone(),
            "Timed out checking node RPC responsiveness",
            source.clone(),
            Some(format!("timeout_secs={}", NODE_LIVENESS_TIMEOUT.as_secs())),
        ),
    }

    let metric_name = "process_start_time_seconds";
    let fetch = async {
        let uri = node_prometheus_uri(node)?;
        let value = fetch_metric_value(&uri, metric_name).await?;
        Ok::<_, anyhow::Error>((uri, value))
    };

    match timeout(NODE_LIVENESS_TIMEOUT, fetch).await {
        Ok(Ok((uri, value))) => evidence::push(
            &mut report,
            Severity::Info,
            format!("node.{node_name}.metric.{metric_name}"),
            Category::Metrics,
            node_name.clone(),
            format!("{metric_name} reported {value}"),
            uri,
            None,
        ),
        Ok(Err(error)) => evidence::push(
            &mut report,
            Severity::Warning,
            format!("node.{node_name}.metric.{metric_name}_failed"),
            Category::Metrics,
            node_name.clone(),
            "Could not query process start time metric",
            source.clone(),
            Some(error.to_string()),
        ),
        Err(_) => evidence::push(
            &mut report,
            Severity::Warning,
            format!("node.{node_name}.metric.{metric_name}_timeout"),
            Category::Metrics,
            node_name.clone(),
            "Timed out querying process start time metric",
            source.clone(),
            Some(format!("timeout_secs={}", NODE_LIVENESS_TIMEOUT.as_secs())),
        ),
    }

    let best_block_metric = "block_height{status=\"best\"}";
    let best_block_fetch = async {
        let uri = node_prometheus_uri(node)?;
        let value = fetch_metric_value(&uri, best_block_metric).await?;
        Ok::<_, anyhow::Error>((uri, value))
    };

    match timeout(NODE_LIVENESS_TIMEOUT, best_block_fetch).await {
        // A node that is up but stuck at block 0 has not produced or imported any
        // blocks; surface it as a warning so a stalled network is not reported as
        // healthy just because its RPC endpoint answers.
        Ok(Ok((uri, 0.0))) => evidence::push(
            &mut report,
            Severity::Warning,
            format!("node.{node_name}.no_block_progress"),
            Category::Liveness,
            node_name,
            "Node is up but its best block is still 0 (no blocks produced or imported)",
            uri,
            None,
        ),
        Ok(Ok((uri, value))) => evidence::push(
            &mut report,
            Severity::Info,
            format!("node.{node_name}.best_block"),
            Category::Liveness,
            node_name,
            format!("Node best block height is {value}"),
            uri,
            None,
        ),
        Ok(Err(error)) => evidence::push(
            &mut report,
            Severity::Info,
            format!("node.{node_name}.best_block_unavailable"),
            Category::Metrics,
            node_name,
            "Best block metric was not available",
            source,
            Some(error.to_string()),
        ),
        Err(_) => evidence::push(
            &mut report,
            Severity::Warning,
            format!("node.{node_name}.best_block_timeout"),
            Category::Metrics,
            node_name,
            "Timed out querying best block metric",
            source,
            Some(format!("timeout_secs={}", NODE_LIVENESS_TIMEOUT.as_secs())),
        ),
    }

    report
}

pub(super) async fn collect_node_logs_for_node(
    node: &NetworkNode,
    source: String,
    lines: usize,
) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("Node log retrieval completed");
    let node_name = node.name().to_string();

    match timeout(NODE_LOGS_TIMEOUT, node.logs()).await {
        Ok(Ok(logs)) => {
            let bounded_logs = bounded_tail_bytes(&logs, MAX_LOG_BYTES);
            let tail = bounded_tail(&bounded_logs, lines, EXCERPT_MAX_BYTES);
            let log_matches = scan_logs(&tail);

            evidence::push(
                &mut report,
                Severity::Info,
                format!("node.{node_name}.logs"),
                Category::Logs,
                node_name.clone(),
                "Node logs were collected",
                source.clone(),
                Some(tail),
            );

            for log_match in log_matches {
                evidence::push(
                    &mut report,
                    log_match.severity,
                    format!("logs.{}", log_match.pattern),
                    log_match.category,
                    node_name.clone(),
                    log_match.message,
                    source.clone(),
                    Some(log_match.line),
                );
            }
        },
        Ok(Err(error)) => evidence::push(
            &mut report,
            Severity::Error,
            format!("node.{node_name}.logs_failed"),
            Category::Logs,
            node_name,
            "Could not read node logs",
            source,
            Some(error.to_string()),
        ),
        Err(_) => evidence::push(
            &mut report,
            Severity::Error,
            format!("node.{node_name}.logs_timeout"),
            Category::Logs,
            node_name,
            "Timed out reading node logs",
            source,
            Some(format!("timeout_secs={}", NODE_LOGS_TIMEOUT.as_secs())),
        ),
    }

    report
}

async fn collect_finalized_blocks(
    node: &NetworkNode,
    blocks: usize,
) -> Result<Vec<String>, anyhow::Error> {
    let client = node.wait_client::<subxt::PolkadotConfig>().await?;
    let mut subscription = client.blocks().subscribe_finalized().await?;
    let mut observed = Vec::with_capacity(blocks);

    while observed.len() < blocks {
        let Some(block) = subscription.next().await else {
            return Err(anyhow::anyhow!(
                "finalized block stream ended after {} of {} produced blocks",
                observed.len(),
                blocks,
            ));
        };

        let number = block?.header().number;
        // The genesis block (#0) is finalized from the start, so observing it is
        // not evidence of ongoing block production. A stalled chain only ever
        // emits its current head once, so requiring `blocks` non-genesis blocks
        // turns a stuck network into a timeout instead of a false success.
        if number > 0 {
            observed.push(format!("Block #{number}"));
        }
    }

    Ok(observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        diagnostics::test_helpers::{fixture_path, temp_zombie_json},
        report::Status,
    };

    #[tokio::test]
    async fn list_nodes_reports_native_attach_error_as_evidence() {
        let path = temp_zombie_json("native-attach-error", r#"{"relay": {"nodes": []}}"#);

        let report = list_nodes(ListNodesInput {
            zombie_json_path: path.path().clone(),
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

    #[tokio::test]
    async fn get_node_logs_reports_invalid_input_as_evidence() {
        let report = get_node_logs(NodeLogsInput {
            zombie_json_path: fixture_path("does-not-matter.json"),
            node_name: "alice".to_string(),
            lines: 0,
        })
        .await;

        assert_eq!(report.status, Status::Failed);
        assert!(report.evidence.iter().any(|e| e.id == "input.invalid"
            && e.severity == Severity::Error
            && e.category == Category::Config));
    }

    #[tokio::test]
    async fn check_block_production_reports_invalid_input_as_evidence() {
        let report = check_block_production(BlockProductionInput {
            zombie_json_path: fixture_path("does-not-matter.json"),
            node_name: "alice".to_string(),
            blocks: 0,
            timeout_secs: 10,
        })
        .await;

        assert_eq!(report.status, Status::Failed);
        assert!(report.evidence.iter().any(|e| e.id == "input.invalid"
            && e.severity == Severity::Error
            && e.category == Category::Config));
    }
}
