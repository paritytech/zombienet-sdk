use std::process::Command;

use serde_json::Value;

fn invalid_config() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/invalid.toml").to_string()
}

/// `validate --config <invalid.toml>` returns a `failed` report carrying the
/// `config.invalid` evidence, and exits 0 without `--fail-on-error`.
#[test]
fn validate_reports_invalid_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_zombie-mcp"))
        .args(["validate", "--config", &invalid_config()])
        .output()
        .expect("zombie-mcp binary runs");

    assert!(
        output.status.success(),
        "expected exit 0 without --fail-on-error, got {:?}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let report: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("stdout should be a JSON report ({error}): {stdout}"));

    assert_eq!(report["status"], "failed", "expected failed status: {stdout}");
    let has_config_invalid = report["evidence"]
        .as_array()
        .expect("evidence is an array")
        .iter()
        .any(|item| item["id"] == "config.invalid");
    assert!(has_config_invalid, "expected config.invalid evidence: {stdout}");
}

/// `--fail-on-error` turns an invalid config into a non-zero exit (CI gate).
#[test]
fn validate_fail_on_error_exits_nonzero() {
    let output = Command::new(env!("CARGO_BIN_EXE_zombie-mcp"))
        .args(["validate", "--config", &invalid_config(), "--fail-on-error"])
        .output()
        .expect("zombie-mcp binary runs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 with --fail-on-error on an invalid config; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}
