use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use walkdir::WalkDir;

const MAX_DEPTH: usize = 6;
const MAX_RUNS: usize = 10;
const RECENT_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecentRunsReport {
    pub status: RecentRunsStatus,
    pub summary: String,
    pub searched_roots: Vec<String>,
    pub runs: Vec<RecentRun>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentRunsStatus {
    Ok,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecentRun {
    pub zombie_json_path: String,
    pub base_dir: String,
    pub modified_unix_secs: u64,
    pub age_secs: u64,
    pub source: RunSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    ZombieJson,
    LogDirectory,
}

pub fn find_recent_runs() -> RecentRunsReport {
    find_recent_runs_in_roots(default_search_roots())
}

fn default_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }

    roots.push(std::env::temp_dir());
    roots.push(PathBuf::from("/tmp"));
    roots.sort();
    roots.dedup();
    roots
}

fn find_recent_runs_in_roots(roots: Vec<PathBuf>) -> RecentRunsReport {
    let now = SystemTime::now();
    let mut seen = HashSet::new();
    let mut runs = roots
        .iter()
        .flat_map(|root| discover_runs(root, now, is_tempish_root(root)))
        .filter(|run| seen.insert(run.zombie_json_path.clone()))
        .collect::<Vec<_>>();

    runs.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));
    runs.truncate(MAX_RUNS);

    RecentRunsReport {
        status: if runs.is_empty() {
            RecentRunsStatus::NotFound
        } else {
            RecentRunsStatus::Ok
        },
        summary: if runs.is_empty() {
            "No recent zombienet runs were found".to_string()
        } else {
            format!("Found {} recent zombienet run candidate(s)", runs.len())
        },
        searched_roots: roots
            .into_iter()
            .map(|root| root.display().to_string())
            .collect(),
        runs,
        next_steps: vec![
            "Use the newest zombie_json_path with diagnose_run".to_string(),
            "If no runs are found, ask the user for zombie_json_path".to_string(),
        ],
    }
}

fn discover_runs(root: &Path, now: SystemTime, allow_log_dir_candidates: bool) -> Vec<RecentRun> {
    if root.is_file() {
        return fs::metadata(root)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| zombie_json_run(root, modified, now))
            .into_iter()
            .collect();
    }

    WalkDir::new(root)
        .max_depth(MAX_DEPTH)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            let modified = entry.metadata().ok()?.modified().ok()?;

            if path.file_name().is_some_and(|name| name == "zombie.json") {
                return zombie_json_run(path, modified, now);
            }

            if allow_log_dir_candidates && path.extension().is_some_and(|ext| ext == "log") {
                let run_dir = zombienet_ancestor(path.parent()?, root)?;
                return log_dir_run(&run_dir, modified, now);
            }

            None
        })
        .collect()
}

fn zombie_json_run(path: &Path, modified: SystemTime, now: SystemTime) -> Option<RecentRun> {
    push_run(path.to_path_buf(), modified, now, RunSource::ZombieJson)
}

fn log_dir_run(dir: &Path, modified: SystemTime, now: SystemTime) -> Option<RecentRun> {
    push_run(
        dir.join("zombie.json"),
        modified,
        now,
        RunSource::LogDirectory,
    )
}

fn push_run(
    zombie_json_path: PathBuf,
    modified: SystemTime,
    now: SystemTime,
    source: RunSource,
) -> Option<RecentRun> {
    if !is_recent(modified, now) {
        return None;
    }

    let base_dir = zombie_json_path.parent().unwrap_or_else(|| Path::new("."));
    Some(RecentRun {
        zombie_json_path: zombie_json_path.display().to_string(),
        base_dir: base_dir.display().to_string(),
        modified_unix_secs: unix_secs(modified),
        age_secs: now
            .duration_since(modified)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs(),
        source,
    })
}

fn zombienet_ancestor(path: &Path, root: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if looks_like_zombienet_dir(ancestor) {
            return Some(ancestor.to_path_buf());
        }
        if ancestor == root {
            break;
        }
    }

    None
}

fn looks_like_zombienet_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("zombie") || name.contains("zombienet"))
}

fn is_tempish_root(root: &Path) -> bool {
    root == Path::new("/tmp") || root.starts_with(std::env::temp_dir())
}

fn is_recent(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .unwrap_or_else(|_| Duration::from_secs(0))
        <= RECENT_WINDOW
}

fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ))
    }

    #[test]
    fn finds_recent_zombie_json() {
        let root = unique_temp_path("zombie-mcp-recent-runs-json");
        let run_dir = root.join("run");
        fs::create_dir_all(&run_dir).expect("run dir fixture can be created");
        fs::write(run_dir.join("zombie.json"), "{}").expect("zombie.json fixture can be written");

        let report = find_recent_runs_in_roots(vec![root.clone()]);

        fs::remove_dir_all(&root).expect("run dir fixture can be removed");

        assert_eq!(report.status, RecentRunsStatus::Ok);
        assert_eq!(report.runs.len(), 1);
        assert!(report.runs[0].zombie_json_path.ends_with("zombie.json"));
        assert_eq!(report.runs[0].source, RunSource::ZombieJson);
    }

    #[test]
    fn synthesizes_zombie_json_path_for_recent_zombie_log_dir() {
        let root = unique_temp_path("zombie-mcp-recent-runs-log-dir");
        let run_dir = root.join("zombienet-sdk-run");
        let node_dir = run_dir.join("alice");
        fs::create_dir_all(&node_dir).expect("node dir fixture can be created");
        fs::write(node_dir.join("alice.log"), "error").expect("log fixture can be written");

        let report = find_recent_runs_in_roots(vec![root.clone()]);

        fs::remove_dir_all(&root).expect("run dir fixture can be removed");

        assert_eq!(report.status, RecentRunsStatus::Ok);
        assert_eq!(report.runs.len(), 1);
        assert!(report.runs[0]
            .zombie_json_path
            .ends_with("zombienet-sdk-run/zombie.json"));
        assert_eq!(report.runs[0].source, RunSource::LogDirectory);
    }

    #[test]
    fn ignores_plain_log_dirs() {
        let root = unique_temp_path("plain-recent-runs-log-dir");
        let run_dir = root.join("plain-run");
        fs::create_dir_all(&run_dir).expect("run dir fixture can be created");
        fs::write(run_dir.join("alice.log"), "error").expect("log fixture can be written");

        let report = find_recent_runs_in_roots(vec![root.clone()]);

        fs::remove_dir_all(&root).expect("run dir fixture can be removed");

        assert_eq!(report.status, RecentRunsStatus::NotFound);
        assert!(report.runs.is_empty());
    }
}
