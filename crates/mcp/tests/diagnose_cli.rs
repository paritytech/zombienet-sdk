use std::process::Command;

use serde_json::Value;

/// `diagnose --zombie-json <nonexistent>` returns a report (not a panic) with the
/// `zombie_json.missing` warning and exits 0 without `--fail-on-error`.
#[test]
fn diagnose_missing_zombie_json_reports_and_exits_zero() {
    let missing = std::env::temp_dir().join(format!(
        "zombie-mcp-diagnose-cli-missing-{}/zombie.json",
        std::process::id(),
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_zombie-mcp"))
        .args(["diagnose", "--zombie-json", &missing.display().to_string()])
        .output()
        .expect("zombie-mcp binary runs");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let report: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("stdout should be a JSON report ({error}): {stdout}"));

    let has_missing = report["evidence"]
        .as_array()
        .expect("evidence is an array")
        .iter()
        .any(|item| item["id"] == "zombie_json.missing");
    assert!(has_missing);
}

/// Passing neither `--zombie-json` nor `--auto` is a usage error (non-zero exit).
#[test]
fn diagnose_without_path_or_auto_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_zombie-mcp"))
        .arg("diagnose")
        .output()
        .expect("zombie-mcp binary runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("pass --zombie-json <path> or --auto"));
}

/// The CI gate: a run whose `zombie.json` has no attachable nodes yields a
/// `failed` report. `--fail-on-error` must turn that into exit 1, while the same
/// run without the flag still exits 0 (the report is always printed either way).
#[test]
fn fail_on_error_flips_exit_code_for_failed_run() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!(
        "zombie-mcp-diagnose-cli-failed-{}",
        std::process::id(),
    ));
    fs::create_dir_all(&dir).expect("temp run dir can be created");
    let zombie_json = dir.join("zombie.json");
    // No attachable nodes -> network.attach_failed (Error) -> status "failed".
    fs::write(&zombie_json, r#"{"relay": {"nodes": []}}"#).expect("zombie.json can be written");
    let path = zombie_json.display().to_string();

    // Without the flag: report is printed and the status really is `failed`, but
    // the process still exits 0 — so the flag has a non-trivial effect to verify.
    let baseline = Command::new(env!("CARGO_BIN_EXE_zombie-mcp"))
        .args(["diagnose", "--zombie-json", &path])
        .output()
        .expect("zombie-mcp binary runs");
    let stdout = String::from_utf8(baseline.stdout).expect("stdout is utf-8");
    let report: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("stdout should be a JSON report ({error}): {stdout}"));
    assert_eq!(report["status"], "failed", "expected failed status: {stdout}");
    assert!(
        baseline.status.success(),
        "expected exit 0 without --fail-on-error, got {:?}",
        baseline.status,
    );

    // With the flag: the same failed report exits 1.
    let gated = Command::new(env!("CARGO_BIN_EXE_zombie-mcp"))
        .args(["diagnose", "--zombie-json", &path, "--fail-on-error"])
        .output()
        .expect("zombie-mcp binary runs");

    fs::remove_dir_all(&dir).ok();

    assert_eq!(
        gated.status.code(),
        Some(1),
        "expected exit 1 with --fail-on-error on a failed run; stderr: {}",
        String::from_utf8_lossy(&gated.stderr),
    );
}
