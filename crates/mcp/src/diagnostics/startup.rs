use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use zombienet_sdk::NetworkConfig;

use super::evidence;
use crate::{
    input::{ConfigInput, DiagnoseRunInput},
    log_patterns::scan_logs,
    report::{bounded_tail, Category, DiagnosticReport, Severity},
};

pub(super) const MAX_LOG_BYTES: usize = 1024 * 1024;
const DIAGNOSE_LOG_LINES: usize = 200;
const MAX_DISCOVERED_LOG_FILES: usize = 64;
const MAX_LOG_DISCOVERY_DEPTH: usize = 4;

pub fn validate_config(input: ConfigInput) -> DiagnosticReport {
    match load_config(&input.config_path) {
        Ok(config) => {
            let summary = summarize_valid_config(&config);
            let mut report = DiagnosticReport::new(summary.clone());

            evidence::push(
                &mut report,
                Severity::Info,
                "config.valid",
                Category::Config,
                input.config_path.display().to_string(),
                summary,
                input.config_path.display().to_string(),
                None,
            );

            evidence::finalize(&mut report);
            report
        },
        Err(error) => {
            let mut report = DiagnosticReport::new("Configuration is invalid");
            evidence::push(
                &mut report,
                Severity::Error,
                "config.invalid",
                Category::Config,
                input.config_path.display().to_string(),
                "Configuration could not be loaded",
                input.config_path.display().to_string(),
                Some(error.to_string()),
            );
            evidence::finalize(&mut report);
            report
        },
    }
}

pub(super) fn diagnose_startup_files(input: &DiagnoseRunInput) -> DiagnosticReport {
    let mut report = DiagnosticReport::new("No startup diagnostics were found");

    let zombie_json_path = &input.zombie_json_path;
    let (id, severity, message) = if zombie_json_path.exists() {
        (
            "zombie_json.exists",
            Severity::Info,
            "zombie.json file exists",
        )
    } else {
        (
            "zombie_json.missing",
            Severity::Warning,
            "zombie.json file was not found",
        )
    };

    evidence::push(
        &mut report,
        severity,
        id,
        Category::Startup,
        zombie_json_path.display().to_string(),
        message,
        zombie_json_path.display().to_string(),
        None,
    );

    let log_paths = discover_log_paths(zombie_json_path);
    if log_paths.is_empty() {
        evidence::push(
            &mut report,
            Severity::Info,
            "logs.none_discovered",
            Category::Logs,
            zombie_json_path.display().to_string(),
            "No log files were discovered from zombie.json or its base directory",
            zombie_json_path.display().to_string(),
            None,
        );
    }

    for log_path in log_paths {
        scan_log_file(&mut report, &log_path, DIAGNOSE_LOG_LINES);
    }

    evidence::finalize(&mut report);
    report
}

fn scan_log_file(report: &mut DiagnosticReport, log_path: &Path, log_lines: usize) {
    const EXCERPT_MAX_BYTES: usize = 8 * 1024;

    match read_bounded_file(log_path, MAX_LOG_BYTES) {
        Ok(logs) => {
            let tail = bounded_tail(&logs, log_lines, EXCERPT_MAX_BYTES);
            for log_match in scan_logs(&tail) {
                evidence::push(
                    report,
                    log_match.severity,
                    format!("logs.{}", log_match.pattern),
                    log_match.category,
                    log_path.display().to_string(),
                    log_match.message,
                    log_path.display().to_string(),
                    Some(log_match.line),
                );
            }
        },
        Err(error) => {
            evidence::push(
                report,
                Severity::Warning,
                "logs.unreadable",
                Category::Logs,
                log_path.display().to_string(),
                "Log file could not be read",
                log_path.display().to_string(),
                Some(error.to_string()),
            );
        },
    }
}

pub(super) fn read_bounded_file(path: &Path, max_bytes: usize) -> Result<String, anyhow::Error> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > max_bytes as u64 {
        file.seek(SeekFrom::Start(len - max_bytes as u64))?;
    }

    let mut bytes = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn discover_log_paths(zombie_json_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if zombie_json_path.is_file() {
        if let Ok(log_paths) = log_paths_from_zombie_json(zombie_json_path) {
            paths.extend(log_paths);
        }
    }

    if paths.is_empty() {
        if let Some(base_dir) = zombie_json_path.parent() {
            discover_log_files(base_dir, 0, &mut paths);
        }
    }

    paths.sort();
    paths.dedup();
    paths.truncate(MAX_DISCOVERED_LOG_FILES);
    paths
}

fn log_paths_from_zombie_json(zombie_json_path: &Path) -> Result<Vec<PathBuf>, anyhow::Error> {
    let contents = read_bounded_file(zombie_json_path, MAX_LOG_BYTES)?;
    let value: serde_json::Value = serde_json::from_str(&contents)?;
    let base_dir = zombie_json_path.parent().unwrap_or_else(|| Path::new("."));
    let mut paths = Vec::new();

    collect_log_paths_from_value(&value, base_dir, &mut paths);
    Ok(paths)
}

fn collect_log_paths_from_value(
    value: &serde_json::Value,
    base_dir: &Path,
    paths: &mut Vec<PathBuf>,
) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if key == "log_path" {
                    if let Some(path) = value.as_str() {
                        push_log_path(paths, base_dir, path);
                    }
                }
                collect_log_paths_from_value(value, base_dir, paths);
            }
        },
        serde_json::Value::Array(items) => {
            for item in items {
                collect_log_paths_from_value(item, base_dir, paths);
            }
        },
        _ => {},
    }
}

fn push_log_path(paths: &mut Vec<PathBuf>, base_dir: &Path, path: &str) {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        paths.push(path);
    } else {
        paths.push(base_dir.join(path));
    }
}

fn discover_log_files(dir: &Path, depth: usize, paths: &mut Vec<PathBuf>) {
    if depth > MAX_LOG_DISCOVERY_DEPTH || paths.len() >= MAX_DISCOVERED_LOG_FILES {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if paths.len() >= MAX_DISCOVERED_LOG_FILES {
            return;
        }

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();

        if file_type.is_dir() {
            discover_log_files(&path, depth + 1, paths);
        } else if file_type.is_file()
            && path.extension().is_some_and(|extension| extension == "log")
        {
            paths.push(path);
        }
    }
}

fn load_config(path: &Path) -> Result<NetworkConfig, anyhow::Error> {
    NetworkConfig::load_from_toml(&path.to_string_lossy())
}

fn summarize_valid_config(config: &NetworkConfig) -> String {
    let relaychain_nodes = config.relaychain().nodes().len();
    let parachains = config.parachains();
    let parachain_count = parachains.len();
    let collator_count = parachains
        .iter()
        .map(|parachain| parachain.collators().len())
        .sum::<usize>();
    let custom_process_count = config.custom_processes().len();

    format!(
        "Configuration is valid: relaychain={:?}, relaychain_nodes={}, parachains={}, collators={}, custom_processes={}",
        config.relaychain().chain(),
        relaychain_nodes,
        parachain_count,
        collator_count,
        custom_process_count,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{
        diagnostics::test_helpers::{fixture_path, unique_temp_path},
        report::Status,
    };

    fn startup_input(zombie_json_path: PathBuf) -> DiagnoseRunInput {
        DiagnoseRunInput { zombie_json_path }
    }

    #[test]
    fn invalid_config_returns_error_evidence() {
        let report = validate_config(ConfigInput {
            config_path: fixture_path("invalid.toml"),
        });

        assert_eq!(report.status, Status::Failed);
        assert!(report.evidence.iter().any(|e| e.id == "config.invalid"
            && e.severity == Severity::Error
            && e.category == Category::Config));
    }

    #[test]
    fn startup_logs_are_discovered_when_zombie_json_is_missing() {
        let base_dir = unique_temp_path("zombie-mcp-startup-log-discovery", "dir");
        let node_dir = base_dir.join("alice");
        fs::create_dir_all(&node_dir).expect("node dir fixture can be created");
        fs::copy(
            fixture_path("startup-failure.log"),
            node_dir.join("alice.log"),
        )
        .expect("log fixture can be copied");

        let input = startup_input(base_dir.join("zombie.json"));

        let report = diagnose_startup_files(&input);

        fs::remove_dir_all(&base_dir).expect("base dir fixture can be removed");

        assert_eq!(report.status, Status::Failed);
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "logs.address already in use" && e.severity == Severity::Error));
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "logs.panicked at" && e.severity == Severity::Error));
    }

    #[test]
    fn startup_logs_are_discovered_from_zombie_json() {
        let base_dir = unique_temp_path("zombie-mcp-zombie-json-log-discovery", "dir");
        let node_dir = base_dir.join("alice");
        fs::create_dir_all(&node_dir).expect("node dir fixture can be created");
        let log_path = node_dir.join("alice.log");
        fs::copy(fixture_path("startup-failure.log"), &log_path)
            .expect("log fixture can be copied");
        let zombie_json_path = base_dir.join("zombie.json");
        fs::write(
            &zombie_json_path,
            format!(
                r#"{{"relay": {{"nodes": [{{"name": "alice", "log_path": "{}"}}]}}}}"#,
                log_path.display()
            ),
        )
        .expect("zombie.json fixture can be written");

        let report = diagnose_startup_files(&startup_input(zombie_json_path));

        fs::remove_dir_all(&base_dir).expect("base dir fixture can be removed");

        assert_eq!(report.status, Status::Failed);
        assert!(report.evidence.iter().any(|e| e.id == "zombie_json.exists"
            && e.severity == Severity::Info
            && e.category == Category::Startup));
        assert!(report
            .evidence
            .iter()
            .any(|e| e.id == "logs.address already in use" && e.severity == Severity::Error));
    }

    #[test]
    fn bounded_file_read_limits_large_logs() {
        let path = unique_temp_path("zombie-mcp-bounded-log", "log");
        fs::write(&path, "a".repeat(MAX_LOG_BYTES + 4096)).expect("log fixture can be written");

        let bounded = read_bounded_file(&path, MAX_LOG_BYTES).expect("log fixture can be read");

        fs::remove_file(path).expect("log fixture can be removed");

        assert!(bounded.len() <= MAX_LOG_BYTES);
    }

    #[test]
    fn log_scanning_respects_bounded_tail() {
        let path = unique_temp_path("zombie-mcp-tail", "log");
        fs::write(
            &path,
            "older panicked at startup\nrecent informational line\nfinal line",
        )
        .expect("log fixture can be written");

        let mut report = DiagnosticReport::new("test");
        scan_log_file(&mut report, &path, 2);

        fs::remove_file(path).expect("log fixture can be removed");

        assert!(!report.evidence.iter().any(|e| e.id == "logs.panicked at"));
    }
}
