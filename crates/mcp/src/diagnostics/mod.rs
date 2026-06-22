mod evidence;
mod live;
mod metrics;
mod nodes;
mod startup;

#[cfg(test)]
mod test_helpers;

pub use metrics::query_metric;
pub use nodes::{check_block_production, check_node_liveness, get_node_logs, list_nodes};
pub use startup::validate_config;

use crate::{
    input::DiagnoseRunInput,
    report::{Category, DiagnosticReport, Severity},
};

const DIAGNOSE_LOG_LINES: usize = 300;

pub async fn diagnose_run(input: DiagnoseRunInput) -> DiagnosticReport {
    let mut report = startup::diagnose_startup_files(&input);

    if !input.zombie_json_path.is_file() {
        finalize_run(&mut report);
        return report;
    }

    let Some(network) = live::open_network(&mut report, &input.zombie_json_path).await else {
        finalize_run(&mut report);
        return report;
    };

    let source = input.zombie_json_path.display().to_string();

    let mut live_nodes = network.nodes();
    live_nodes.sort_by(|left, right| left.name().cmp(right.name()));
    for node in live_nodes {
        let node_report = nodes::check_node_liveness_for_node(node, source.clone()).await;
        report.evidence.extend(node_report.evidence);
    }

    let mut collators = network
        .parachains()
        .into_iter()
        .flat_map(|parachain| parachain.collators())
        .collect::<Vec<_>>();
    collators.sort_by(|left, right| left.name().cmp(right.name()));
    for collator in collators {
        let logs_report =
            nodes::collect_node_logs_for_node(collator, source.clone(), DIAGNOSE_LOG_LINES).await;
        report.evidence.extend(logs_report.evidence);
    }

    finalize_run(&mut report);
    report
}

fn finalize_run(report: &mut DiagnosticReport) {
    evidence::finalize(report);
    if report.next_steps.is_empty() {
        report.next_steps = next_steps_for(report);
    }
}

pub fn next_steps_for(report: &DiagnosticReport) -> Vec<String> {
    if report.evidence.iter().any(|item| {
        item.severity == Severity::Error
            && (item.category == Category::Startup
                || item.id.starts_with("config.")
                || item.id.starts_with("logs."))
    }) {
        return vec!["Fix the startup error and rerun the same command".to_string()];
    }

    if report
        .evidence
        .iter()
        .any(|item| item.severity == Severity::Error && item.category == Category::Rpc)
    {
        return vec!["Inspect node logs and verify the RPC port is reachable".to_string()];
    }

    if report
        .evidence
        .iter()
        .any(|item| item.severity == Severity::Error && item.category == Category::Parachain)
    {
        return vec![
            "Check parachain registration, collator status, and relay finalization".to_string(),
        ];
    }

    if report
        .evidence
        .iter()
        .any(|item| item.id.ends_with(".no_block_progress"))
    {
        return vec![
            "A node is up but not producing or importing blocks. Check parachain registration, \
             collator-to-relay connectivity (peer count), and that the registered para id matches \
             the chain spec."
                .to_string(),
        ];
    }

    vec!["No immediate failure detected".to_string()]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{
        diagnostics::test_helpers::{fixture_path, temp_zombie_json, unique_temp_path},
        report::{Evidence, Status},
    };

    #[test]
    fn next_steps_flags_a_node_stuck_with_no_block_progress() {
        let mut report = DiagnosticReport::new("test");
        report.push_evidence(Evidence {
            id: "node.people-collator.no_block_progress".to_string(),
            severity: Severity::Warning,
            category: Category::Liveness,
            subject: "people-collator".to_string(),
            message: "Node is up but its best block is still 0".to_string(),
            source: "http://127.0.0.1:9615/metrics".to_string(),
            excerpt: None,
        });

        let steps = next_steps_for(&report);

        assert_eq!(steps.len(), 1);
        assert_ne!(steps[0], "No immediate failure detected");
        assert!(steps[0].contains("not producing or importing blocks"));
        assert!(steps[0].contains("para id"));
    }

    #[tokio::test]
    async fn scans_logs_next_to_missing_zombie_json() {
        let base_dir = unique_temp_path("zombie-mcp-missing-json-log-scan", "dir");
        let node_dir = base_dir.join("alice");
        fs::create_dir_all(&node_dir).expect("node dir fixture can be created");
        fs::copy(
            fixture_path("startup-failure.log"),
            node_dir.join("alice.log"),
        )
        .expect("log fixture can be copied");

        let report = diagnose_run(DiagnoseRunInput {
            zombie_json_path: base_dir.join("zombie.json"),
        })
        .await;

        fs::remove_dir_all(&base_dir).expect("base dir fixture can be removed");

        assert_eq!(report.status, Status::Failed);
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "zombie_json.missing" && e.severity == Severity::Warning));
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "logs.address already in use" && e.severity == Severity::Error));
        assert!(!report
            .evidence
            .iter()
            .any(|e| e.id == "network.attach_failed"));
        assert_eq!(
            report.next_steps,
            vec!["Fix the startup error and rerun the same command"]
        );
    }

    #[tokio::test]
    async fn reports_native_attach_error_as_evidence() {
        let path = temp_zombie_json("run-native-attach-error", r#"{"relay": {"nodes": []}}"#);

        let report = diagnose_run(DiagnoseRunInput {
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
        assert_eq!(
            report.next_steps,
            vec!["Fix the startup error and rerun the same command"]
        );
    }
}
