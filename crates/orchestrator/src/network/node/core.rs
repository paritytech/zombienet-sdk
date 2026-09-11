//! Provider-generic node behaviour, shared by every node type.
//!
//! [`NodeCore`] wraps the handle to the running process (the provider's
//! [`DynNode`]) plus the small amount of runtime state zombienet keeps about
//! it, and implements everything that doesn't depend on _what_ the node is
//! running: logs, lifecycle (pause/resume/restart), scripts and db snapshots.
//!
//! Chain-flavoured nodes ([`NetworkNode`](super::NetworkNode) for substrate,
//! [`JamNetworkNode`](super::jam::JamNetworkNode) for JAM) embed one of these
//! and add their own protocol-specific surface on top.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use configuration::types::AssetLocation;
use fancy_regex::Regex;
use glob_match::glob_match;
use provider::{
    types::{ExecutionResult, InnerSnapshotDb, RunScriptOptions},
    DynNode,
};
use serde::Serialize;
use tracing::{debug, trace};

use super::{
    serialize_provider_node, spawned::NodeKind, BoxedClosure, LogLineCount, LogLineCountOptions,
};
use crate::shared::types::NodeSnapshot;

/// The provider handle and runtime state every spawned node has.
///
/// Cloning a `NodeCore` shares the same underlying process handle _and_ the
/// same running/last-start state, so all the clones of a node stay in sync.
#[derive(Clone, Serialize)]
pub struct NodeCore {
    #[serde(serialize_with = "serialize_provider_node")]
    pub(crate) inner: DynNode,
    pub(crate) name: String,
    /// What flavour of node this is. Stored (and serialized to `zombie.json`)
    /// so a node can be identified without its concrete type at hand.
    pub(crate) kind: NodeKind,
    #[serde(skip)]
    is_running: Arc<AtomicBool>,
    // Store the last timestamp when we start the node
    #[serde(skip)]
    last_start_ts: Arc<AtomicU64>,
}

impl NodeCore {
    pub(crate) fn new(name: impl Into<String>, inner: DynNode, kind: NodeKind) -> Self {
        Self {
            inner,
            name: name.into(),
            kind,
            is_running: Arc::new(AtomicBool::new(false)),
            last_start_ts: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The provider node (the actual running process/pod/container).
    pub fn inner(&self) -> &DynNode {
        &self.inner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// What flavour of node this is.
    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    /// Args used for bootstrap the node.
    /// NOTE: this may not be in sync if you restart the node with new args.
    pub fn args(&self) -> Vec<&str> {
        self.inner.args()
    }

    /// Check if the node is currently running (not paused).
    ///
    /// This returns the internal running state.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Get the last timestamp when the node start.
    pub fn last_start_ts(&self) -> u64 {
        self.last_start_ts.load(Ordering::Acquire)
    }

    pub(crate) fn set_is_running(&self, is_running: bool) {
        self.is_running.store(is_running, Ordering::Release);
    }

    /// Set the timestamp when the node was started
    pub(crate) fn set_last_start_ts(&self, ts: u64) {
        self.last_start_ts.store(ts, Ordering::Release);
    }

    /// On-disk base directory of the node — root of `data/`, `relay-data/`,
    /// `cfg/`, etc.
    /// This will be the _base directory_ of the inner (provider) node.
    pub fn base_dir(&self) -> &PathBuf {
        self.inner.base_dir()
    }

    /// Pause the node, this is implemented by pausing the
    /// actual process (e.g polkadot) with sending `SIGSTOP` signal
    ///
    /// Note: If you're using this method with the native provider on the attached network, the live network has to be running
    /// with global setting `teardown_on_failure` disabled.
    pub async fn pause(&self) -> Result<(), anyhow::Error> {
        self.set_is_running(false);
        self.inner.pause().await?;
        Ok(())
    }

    /// Resume the node, this is implemented by resuming the
    /// actual process (e.g polkadot) with sending `SIGCONT` signal
    ///
    /// Note: If you're using this method with the native provider on the attached network, the live network has to be running
    /// with global setting `teardown_on_failure` disabled.
    pub async fn resume(&self) -> Result<(), anyhow::Error> {
        self.set_is_running(true);
        self.inner.resume().await?;
        Ok(())
    }

    /// Restart the node using the same `cmd`, `args` and `env` (and same isolated dir)
    ///
    /// Note: If you're using this method with the native provider on the attached network, the live network has to be running
    /// with global setting `teardown_on_failure` disabled.
    pub async fn restart(&self, after: Option<Duration>) -> Result<(), anyhow::Error> {
        self.set_is_running(false);
        self.inner.restart(after).await?;
        self.set_is_running(true);
        self.set_last_start_ts(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
        Ok(())
    }

    /// Get the logs of the node
    /// TODO: do we need the `since` param, maybe we could be handy later for loop filtering
    pub async fn logs(&self) -> Result<String, anyhow::Error> {
        Ok(self.inner.logs().await?)
    }

    /// Wait until a the number of matching log lines is reach
    pub async fn wait_log_line_count(
        &self,
        pattern: impl Into<String>,
        is_glob: bool,
        count: usize,
    ) -> Result<(), anyhow::Error> {
        let pattern = pattern.into();
        let pattern_clone = pattern.clone();
        debug!("waiting until we find pattern {pattern} {count} times");
        let match_fn: BoxedClosure = if is_glob {
            Box::new(move |line: &str| Ok(glob_match(&pattern, line)))
        } else {
            let re = Regex::new(&pattern)?;
            Box::new(move |line: &str| re.is_match(line).map_err(|e| anyhow!(e.to_string())))
        };

        loop {
            let mut q = 0_usize;
            let logs = self.logs().await?;
            for line in logs.lines() {
                trace!("line is {line}");
                if match_fn(line)? {
                    trace!("pattern {pattern_clone} match in line {line}");
                    q += 1;
                    if q >= count {
                        return Ok(());
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Waits until the number of matching log lines satisfies a custom condition,
    /// optionally waiting for the entire duration of the timeout.
    ///
    /// This method searches log lines for a given substring or glob pattern,
    /// and evaluates the number of matching lines using a user-provided predicate function.
    /// Optionally, it can wait for the full timeout duration to ensure the condition
    /// holds consistently (e.g., for verifying absence of logs).
    ///
    /// # Arguments
    /// * `substring` - The substring or pattern to match within log lines.
    /// * `is_glob` - Whether to treat `substring` as a glob pattern (`true`) or a regex (`false`).
    /// * `options` - Configuration for timeout, match count predicate, and full-duration waiting.
    ///
    /// # Returns
    /// * `Ok(LogLineCount::TargetReached(n))` if the predicate was satisfied within the timeout,
    /// * `Ok(LogLineCount::TargetFails(n))` if the predicate was not satisfied in time,
    /// * `Err(e)` if an error occurred during log retrieval or matching.
    ///
    /// # Example
    /// ```rust
    /// # use std::{sync::Arc, time::Duration};
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem};
    /// # use zombienet_orchestrator::{Orchestrator, network::node::{NetworkNode, LogLineCountOptions}};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), anyhow::Error> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    /// #   let network = orchestrator.spawn(config).await?;
    /// let node = network.get_node("alice")?;
    /// // Wait (up to 10 seconds) until pattern occurs once
    /// let options = LogLineCountOptions {
    ///     predicate: Arc::new(|count| count == 1),
    ///     timeout: Duration::from_secs(10),
    ///     wait_until_timeout_elapses: false,
    /// };
    /// let result = node
    ///     .wait_log_line_count_with_timeout("error", false, options)
    ///     .await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn wait_log_line_count_with_timeout(
        &self,
        substring: impl Into<String>,
        is_glob: bool,
        options: LogLineCountOptions,
    ) -> Result<LogLineCount, anyhow::Error> {
        let substring = substring.into();
        debug!(
            "waiting until match lines count within {} seconds",
            options.timeout.as_secs_f64()
        );

        let start = tokio::time::Instant::now();

        let match_fn: BoxedClosure = if is_glob {
            Box::new(move |line: &str| Ok(glob_match(&substring, line)))
        } else {
            let re = Regex::new(&substring)?;
            Box::new(move |line: &str| re.is_match(line).map_err(|e| anyhow!(e.to_string())))
        };

        if options.wait_until_timeout_elapses {
            tokio::time::sleep(options.timeout).await;
        }

        let mut q;
        loop {
            q = 0_u32;
            let logs = self.logs().await?;
            for line in logs.lines() {
                if match_fn(line)? {
                    q += 1;

                    // If `wait_until_timeout_elapses` is set then check the condition just once at the
                    // end after the whole log file is processed. This is to address the cases when the
                    // predicate becomes true and false again.
                    // eg. expected exactly 2 matching lines are expected but 3 are present
                    if !options.wait_until_timeout_elapses && (options.predicate)(q) {
                        return Ok(LogLineCount::TargetReached(q));
                    }
                }
            }

            if start.elapsed() >= options.timeout {
                break;
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        if (options.predicate)(q) {
            Ok(LogLineCount::TargetReached(q))
        } else {
            Ok(LogLineCount::TargetFailed(q))
        }
    }

    /// Restart the node overriding the program and/or args, optionally
    /// downloading `assets` (binaries) first.
    ///
    /// The node keeps the same `env` and isolated directory (e.g same database).
    /// Callers are expected to have (re)generated `program`/`args` for their
    /// own node flavour.
    pub(crate) async fn restart_with(
        &self,
        assets: &[AssetLocation],
        program: &str,
        args: &[String],
        after: Option<Duration>,
    ) -> Result<(), anyhow::Error> {
        self.set_is_running(false);
        self.inner
            .restart_with(assets, program, args, after)
            .await?;
        self.set_is_running(true);
        self.set_last_start_ts(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
        Ok(())
    }

    /// Tar the node's database into `out_path` (gzipped).
    ///
    /// NOTE: Currently __only__ implemented in native provider. Also,
    /// the caller is responsible for pausing the node first;
    /// snapshotting a running node risks a torn RocksDB state.
    pub(crate) async fn snapshot_db(
        &self,
        out_path: impl AsRef<Path>,
        is_cumulus_based: bool,
    ) -> Result<NodeSnapshot, anyhow::Error> {
        let out_path = out_path.as_ref().to_path_buf();

        let InnerSnapshotDb {
            filename,
            sha256,
            size,
        } = self.inner.snapshot_db(is_cumulus_based).await?;

        // now we need to _move_ the inner file to the out_path
        let remote_file_path = PathBuf::from(&filename);
        self.inner
            .receive_file(remote_file_path.as_ref(), out_path.as_ref())
            .await?;

        Ok(NodeSnapshot {
            path: out_path,
            sha256,
            size,
            node_name: self.name().into(),
        })
    }

    /// Run a script inside the node's container/environment
    ///
    /// The script will be uploaded to the node, made executable, and executed with
    /// the provided arguments and environment variables.
    ///
    /// Returns `Ok(stdout)` on success, or `Err((exit_status, stderr))` on failure.
    pub async fn run_script(
        &self,
        options: RunScriptOptions,
    ) -> Result<ExecutionResult, anyhow::Error> {
        self.inner
            .run_script(options)
            .await
            .map_err(|e| anyhow!("Failed to run script: {e}"))
    }
}

impl std::fmt::Debug for NodeCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeCore")
            .field("inner", &"inner_skipped")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("is_running", &self.is_running())
            .finish()
    }
}
