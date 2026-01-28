//! Application state and core logic for the TUI.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use orchestrator::network::Network;
use support::fs::local::LocalFileSystem;
use tokio::sync::mpsc;
use zombienet_sdk::AttachToLive;

use crate::{
    logs::LogViewer,
    network::{BlockInfo, NodeInfo, NodeStatus, StorageThresholds},
    watcher::{FileWatcher, WatchEvent},
};

struct RefreshResults {
    statuses: HashMap<String, NodeStatus>,
    block_info: HashMap<String, BlockInfo>,
}

/// The current view/panel focus in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// Node list sidebar is focused.
    #[default]
    Nodes,
    /// Node details panel is focused.
    Details,
    /// Log viewer panel is focused.
    Logs,
}

/// The current input mode for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal navigation mode.
    #[default]
    Normal,
    /// Confirmation dialog (y/n).
    Confirm,
    /// Help overlay is visible.
    Help,
    /// Search input mode for logs.
    Search,
}

/// Core application state for the TUI.
pub struct App {
    /// Whether the application is still running.
    running: bool,
    /// The connected zombienet network (if any).
    network: Option<Arc<Network<LocalFileSystem>>>,
    /// Path to the zombie.json file.
    zombie_json_path: Option<PathBuf>,
    /// Current view/panel focus.
    current_view: View,
    /// Current input mode.
    input_mode: InputMode,
    /// Index of the currently selected node.
    selected_node_index: usize,
    /// Cached node information for display.
    nodes: Vec<NodeInfo>,
    /// Log viewer for the currently selected node.
    log_viewer: LogViewer,
    /// Current search input buffer.
    search_input: String,
    /// Status message to display.
    status_message: Option<String>,
    /// Pending confirmation action.
    pending_action: Option<PendingAction>,
    /// File watcher for log file changes.
    file_watcher: Option<FileWatcher>,
    /// Currently watched log file path.
    watched_log_path: Option<PathBuf>,
    /// Last time node statuses were checked.
    last_status_check: Option<Instant>,
    /// Storage thresholds.
    storage_thresholds: StorageThresholds,
    /// Receiver for background refresh results.
    refresh_rx: mpsc::Receiver<RefreshResults>,
    /// Sender for spawning background refresh tasks.
    refresh_tx: mpsc::Sender<RefreshResults>,
    /// Whether a background refresh is currently in progress.
    refresh_in_progress: bool,
}

/// Actions that require user confirmation.
#[derive(Debug, Clone)]
pub enum PendingAction {
    /// Restart a single node.
    RestartNode(String),
    /// Restart all nodes in the network.
    RestartAllNodes,
    /// Shutdown the entire network.
    ShutdownNetwork,
}

impl PendingAction {
    pub fn confirmation_prompt(&self) -> &'static str {
        "Press 'y' to confirm, 'n' to cancel"
    }
}

impl App {
    /// Create a new App instance.
    pub fn new() -> Self {
        let file_watcher = FileWatcher::new().ok();
        let (refresh_tx, refresh_rx) = mpsc::channel(1);

        Self {
            running: true,
            network: None,
            zombie_json_path: None,
            current_view: View::Nodes,
            input_mode: InputMode::Normal,
            selected_node_index: 0,
            nodes: Vec::new(),
            log_viewer: LogViewer::new(),
            search_input: String::new(),
            status_message: None,
            pending_action: None,
            file_watcher,
            watched_log_path: None,
            last_status_check: None,
            storage_thresholds: StorageThresholds::default(),
            refresh_rx,
            refresh_tx,
            refresh_in_progress: false,
        }
    }

    /// Check if the application is still running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Signal the application to quit.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Get the current view.
    pub fn current_view(&self) -> View {
        self.current_view
    }

    /// Set the current view.
    pub fn set_current_view(&mut self, view: View) {
        self.current_view = view;
    }

    /// Get the current input mode.
    pub fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    /// Get the list of nodes.
    pub fn nodes(&self) -> &[NodeInfo] {
        &self.nodes
    }

    /// Get the currently selected node index.
    pub fn selected_node_index(&self) -> usize {
        self.selected_node_index
    }

    /// Get the currently selected node (if any).
    pub fn selected_node(&self) -> Option<&NodeInfo> {
        self.nodes.get(self.selected_node_index)
    }

    /// Get the log viewer.
    pub fn log_viewer(&self) -> &LogViewer {
        &self.log_viewer
    }

    /// Get the current search input.
    pub fn search_input(&self) -> &str {
        &self.search_input
    }

    /// Get the status message.
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    /// Get the pending action requiring confirmation.
    pub fn pending_action(&self) -> Option<&PendingAction> {
        self.pending_action.as_ref()
    }

    /// Get the network name.
    pub fn network_name(&self) -> Option<&str> {
        self.network.as_ref().map(|n| n.name())
    }

    /// Get the network base directory.
    pub fn network_base_dir(&self) -> Option<&str> {
        self.network.as_ref().and_then(|n| n.base_dir())
    }

    pub fn network(&self) -> Option<&Network<LocalFileSystem>> {
        self.network.as_ref().map(|n| n.as_ref())
    }

    /// Set the zombie.json path for attachment.
    pub fn set_zombie_json_path(&mut self, path: PathBuf) {
        self.zombie_json_path = Some(path);
    }

    /// Set custom storage thresholds.
    pub fn set_storage_thresholds(&mut self, thresholds: StorageThresholds) {
        self.storage_thresholds = thresholds;
    }

    /// Get the current storage thresholds.
    pub fn storage_thresholds(&self) -> &StorageThresholds {
        &self.storage_thresholds
    }

    /// Attach to a running network from the zombie.json file.
    pub async fn attach_to_network(&mut self) -> Result<()> {
        let path = self
            .zombie_json_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No zombie.json path set"))?;

        self.set_status(format!("Attaching to network from {}...", path.display()));

        let network = zombienet_sdk::AttachToLiveNetwork::attach_native(path).await?;

        // Extract node information.
        self.nodes = crate::network::extract_nodes(&network).await;
        self.network = Some(Arc::new(network));

        self.set_status("Connected to network");
        Ok(())
    }

    /// Set a status message.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
    }

    /// Clear the status message.
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Move selection up in the node list.
    pub fn select_previous(&mut self) {
        if !self.nodes.is_empty() {
            self.selected_node_index = self.selected_node_index.saturating_sub(1);
        }
    }

    /// Move selection down in the node list.
    pub fn select_next(&mut self) {
        if !self.nodes.is_empty() {
            self.selected_node_index = (self.selected_node_index + 1).min(self.nodes.len() - 1);
        }
    }

    /// Switch to the next view/panel.
    pub fn next_view(&mut self) {
        self.current_view = match self.current_view {
            View::Nodes => View::Details,
            View::Details => View::Logs,
            View::Logs => View::Nodes,
        };
    }

    /// Switch to the previous view/panel.
    pub fn previous_view(&mut self) {
        self.current_view = match self.current_view {
            View::Nodes => View::Logs,
            View::Details => View::Nodes,
            View::Logs => View::Details,
        };
    }

    /// Toggle help overlay.
    pub fn toggle_help(&mut self) {
        self.input_mode = match self.input_mode {
            InputMode::Help => InputMode::Normal,
            _ => InputMode::Help,
        };
    }

    /// Toggle log follow mode.
    pub fn toggle_log_follow(&mut self) {
        self.log_viewer.toggle_follow();
    }

    /// Scroll logs up.
    pub fn scroll_logs_up(&mut self, amount: usize) {
        self.log_viewer.scroll_up(amount);
    }

    /// Scroll logs down.
    pub fn scroll_logs_down(&mut self, amount: usize) {
        self.log_viewer.scroll_down(amount);
    }

    /// Scroll logs to top.
    pub fn scroll_logs_to_top(&mut self) {
        self.log_viewer.scroll_to_top();
    }

    /// Scroll logs to bottom.
    pub fn scroll_logs_to_bottom(&mut self) {
        self.log_viewer.scroll_to_bottom();
    }

    pub fn start_search(&mut self) {
        self.search_input.clear();
        self.input_mode = InputMode::Search;
    }

    /// Cancel search and return to normal mode.
    pub fn cancel_search(&mut self) {
        self.search_input.clear();
        self.log_viewer.clear_search();
        self.input_mode = InputMode::Normal;
    }

    /// Confirm search and return to normal mode.
    pub fn confirm_search(&mut self) {
        self.log_viewer.search(&self.search_input);
        self.input_mode = InputMode::Normal;
    }

    /// Add character to search input.
    pub fn add_search_char(&mut self, c: char) {
        self.search_input.push(c);
        self.log_viewer.search(&self.search_input);
    }

    /// Remove last character from search input.
    pub fn remove_search_char(&mut self) {
        self.search_input.pop();
        if self.search_input.is_empty() {
            self.log_viewer.clear_search();
        } else {
            self.log_viewer.search(&self.search_input);
        }
    }

    /// Jump to next search match.
    pub fn next_search_match(&mut self) {
        self.log_viewer.next_search_match();
    }

    /// Jump to previous search match.
    pub fn prev_search_match(&mut self) {
        self.log_viewer.prev_search_match();
    }

    /// Request confirmation for an action.
    pub fn request_confirmation(&mut self, action: PendingAction) {
        self.pending_action = Some(action);
        self.input_mode = InputMode::Confirm;
    }

    /// Confirm the pending action.
    pub async fn confirm_action(&mut self) -> Result<()> {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::RestartNode(name) => {
                    self.restart_node(&name).await?;
                },
                PendingAction::RestartAllNodes => {
                    self.restart_all_nodes().await?;
                },
                PendingAction::ShutdownNetwork => {
                    self.shutdown_network().await?;
                },
            }
        }
        self.input_mode = InputMode::Normal;
        Ok(())
    }

    /// Cancel the pending action.
    pub fn cancel_action(&mut self) {
        self.pending_action = None;
        self.input_mode = InputMode::Normal;
    }

    /// Pause the currently selected node.
    pub async fn pause_selected_node(&mut self) -> Result<()> {
        let Some(name) = self
            .nodes
            .get(self.selected_node_index)
            .map(|n| n.name.clone())
        else {
            return Ok(());
        };

        let Some(network) = &self.network else {
            return Ok(());
        };

        let node = network.get_node(&name)?;
        node.pause().await?;

        if let Some(node_info) = self.nodes.get_mut(self.selected_node_index) {
            node_info.status = NodeStatus::Paused;
        }
        self.set_status(format!("Paused node: {}", name));

        Ok(())
    }

    /// Resume the currently selected node.
    pub async fn resume_selected_node(&mut self) -> Result<()> {
        let Some(name) = self
            .nodes
            .get(self.selected_node_index)
            .map(|n| n.name.clone())
        else {
            return Ok(());
        };

        let Some(network) = &self.network else {
            return Ok(());
        };

        let node = network.get_node(&name)?;
        node.resume().await?;

        if let Some(node_info) = self.nodes.get_mut(self.selected_node_index) {
            node_info.status = NodeStatus::Running;
        }
        self.set_status(format!("Resumed node: {}", name));

        Ok(())
    }

    /// Restart a specific node.
    async fn restart_node(&mut self, name: &str) -> Result<()> {
        if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == name) {
            node_info.status = crate::network::NodeStatus::Unknown;
        }

        self.set_status(format!("Restarting {}...", name));

        // Perform the restart.
        if let Some(network) = &self.network {
            let node = network.get_node(name)?;
            node.restart(None).await?;
        } else {
            return Err(anyhow::anyhow!("No network connected"));
        }

        // Verify node is responsive.
        self.set_status(format!("Verifying {} is responsive...", name));
        let is_up = self
            .wait_node_responsive(name, Duration::from_secs(30))
            .await;

        if is_up {
            if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == name) {
                node_info.status = crate::network::NodeStatus::Running;
            }
            self.set_status(format!("Restarted node: {}", name));
        } else {
            if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == name) {
                node_info.status = crate::network::NodeStatus::Unknown;
            }
            self.set_status(format!("Restarted {} but node not responsive", name));
        }

        self.refresh_nodes().await;
        Ok(())
    }

    /// Shutdown the entire network.
    async fn shutdown_network(&mut self) -> Result<()> {
        if let Some(network) = self.network.take() {
            self.refresh_in_progress = false;
            match Arc::try_unwrap(network) {
                Ok(network) => {
                    network.destroy().await?;
                    self.set_status("Network shutdown complete");
                },
                Err(_) => {
                    self.set_status("Cannot shutdown: network still in use");
                    return Ok(());
                },
            }
            self.nodes.clear();
            self.quit();
        }
        Ok(())
    }

    /// Restart all nodes in the network sequentially.
    ///
    /// This method:
    /// 1. Restarts each node using the SDK's restart method
    /// 2. Waits for each node to become responsive before proceeding
    /// 3. Tracks progress and reports errors
    async fn restart_all_nodes(&mut self) -> Result<()> {
        let node_names: Vec<String> = self.nodes.iter().map(|n| n.name.clone()).collect();
        let total = node_names.len();

        if total == 0 {
            self.set_status("No nodes to restart");
            return Ok(());
        }

        let mut successful = 0;
        let mut failed_nodes: Vec<(String, String)> = Vec::new();

        for (i, name) in node_names.iter().enumerate() {
            let progress = format!("[{}/{}]", i + 1, total);

            self.set_status(format!("{} Restarting {}...", progress, name));

            if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == *name) {
                node_info.status = crate::network::NodeStatus::Unknown;
            }

            let restart_result = if let Some(network) = &self.network {
                match network.get_node(name) {
                    Ok(node) => node.restart(None).await,
                    Err(e) => Err(e),
                }
            } else {
                Err(anyhow::anyhow!("No network connected"))
            };

            match restart_result {
                Ok(()) => {
                    // Update status: verifying.
                    self.set_status(format!("{} Verifying {} is responsive...", progress, name));

                    let is_up = self
                        .wait_node_responsive(name, Duration::from_secs(30))
                        .await;

                    if is_up {
                        if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == *name) {
                            node_info.status = crate::network::NodeStatus::Running;
                        }
                        successful += 1;
                    } else {
                        if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == *name) {
                            node_info.status = crate::network::NodeStatus::Unknown;
                        }
                        failed_nodes
                            .push((name.clone(), "Node not responsive after restart".into()));
                    }
                },
                Err(e) => {
                    failed_nodes.push((name.clone(), e.to_string()));
                    if let Some(node_info) = self.nodes.iter_mut().find(|n| n.name == *name) {
                        node_info.status = crate::network::NodeStatus::Unknown;
                    }
                },
            }

            if i < total - 1 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }

        self.refresh_nodes().await;

        if failed_nodes.is_empty() {
            self.set_status(format!("Successfully restarted all {} nodes", successful));
        } else {
            let error_summary: String = failed_nodes
                .iter()
                .map(|(name, err)| format!("{}: {}", name, err))
                .collect::<Vec<_>>()
                .join("; ");
            self.set_status(format!(
                "Restarted {}/{} nodes. Failures: {}",
                successful, total, error_summary
            ));
        }

        Ok(())
    }

    /// Wait for a node to become responsive with timeout.
    ///
    /// Returns true if node becomes responsive within the timeout.
    async fn wait_node_responsive(&self, node_name: &str, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        let check_interval = Duration::from_millis(500);

        while start.elapsed() < timeout {
            if let Some(network) = &self.network {
                if let Ok(node) = network.get_node(node_name) {
                    if node.is_responsive().await {
                        return true;
                    }
                }
            }
            tokio::time::sleep(check_interval).await;
        }

        false
    }

    /// Refresh the node list from the network.
    pub async fn refresh_nodes(&mut self) {
        if let Some(network) = &self.network {
            self.nodes = crate::network::extract_nodes(network).await;
        }
    }

    /// Refresh node statuses by checking RPC connectivity.
    ///
    /// This performs actual connection attempts to verify nodes are responsive.
    /// Call this periodically for accurate status information.
    pub async fn refresh_node_statuses(&mut self) {
        if let Some(network) = &self.network {
            let status_map = crate::network::check_all_nodes_status_async(network).await;

            for node in &mut self.nodes {
                if let Some(status) = status_map.get(&node.name) {
                    node.status = *status;
                }
            }
        }
    }

    /// Refresh block info (best/finalized blocks) for all nodes.
    pub async fn refresh_block_info(&mut self) {
        if let Some(network) = &self.network {
            let block_info_map = crate::network::fetch_all_nodes_block_info(network).await;

            for node in &mut self.nodes {
                if let Some(block_info) = block_info_map.get(&node.name) {
                    node.block_info = Some(block_info.clone());
                }
            }
        }
    }

    /// Check and update status for a single node.
    pub async fn refresh_node_status(&mut self, node_name: &str) {
        if let Some(network) = &self.network {
            let status = crate::network::check_node_status_async(network, node_name).await;

            if let Some(node) = self.nodes.iter_mut().find(|n| n.name == node_name) {
                node.status = status;
            }
        }
    }

    /// Load logs for the currently selected node.
    pub async fn load_selected_node_logs(&mut self) -> Result<()> {
        if let (Some(base_dir), Some(node_info)) = (
            self.network_base_dir().map(String::from),
            self.selected_node(),
        ) {
            let log_path = crate::network::derive_log_path(&base_dir, &node_info.name);

            if self.watched_log_path.as_ref() != Some(&log_path) {
                if let (Some(watcher), Some(old_path)) =
                    (&mut self.file_watcher, &self.watched_log_path)
                {
                    let _ = watcher.unwatch(old_path);
                }

                if let Some(watcher) = &mut self.file_watcher {
                    if log_path.exists() {
                        let _ = watcher.watch(&log_path);
                    }
                }

                self.watched_log_path = Some(log_path.clone());
            }

            self.log_viewer.set_log_path(log_path)?;
        }
        Ok(())
    }

    /// Refresh logs from the current log file.
    pub fn refresh_logs(&mut self) -> Result<()> {
        self.log_viewer.load_from_file()?;
        Ok(())
    }

    /// Periodic tick for async updates.
    pub fn tick(&mut self) {
        let events: Vec<WatchEvent> = self
            .file_watcher
            .as_ref()
            .map(|w| std::iter::from_fn(|| w.try_recv()).collect())
            .unwrap_or_default();

        let mut is_needs_refresh = false;
        for event in events {
            match event {
                WatchEvent::Modified(path) => {
                    if self.watched_log_path.as_ref() == Some(&path) {
                        is_needs_refresh = true;
                    }
                },
                WatchEvent::Error(e) => {
                    self.set_status(format!("Error watching log file: {e}"));
                },
            }
        }

        if is_needs_refresh {
            let _ = self.refresh_logs();
        }

        if self.log_viewer.follow() && self.current_view == View::Logs {
            let _ = self.refresh_logs();
        }

        while let Ok(results) = self.refresh_rx.try_recv() {
            for node in &mut self.nodes {
                if let Some(status) = results.statuses.get(&node.name) {
                    node.status = *status;
                }
                if let Some(block_info) = results.block_info.get(&node.name) {
                    node.block_info = Some(block_info.clone());
                }
            }
            self.refresh_in_progress = false;
        }

        const STATUS_CHECK_INTERVAL: Duration = Duration::from_secs(5);
        let should_check_status = self
            .last_status_check
            .map(|t| t.elapsed() >= STATUS_CHECK_INTERVAL)
            .unwrap_or(true);

        if should_check_status && self.network.is_some() && !self.refresh_in_progress {
            self.spawn_refresh();
            self.last_status_check = Some(Instant::now());
        }
    }

    /// Spawn a task to refresh node statuses and block info.
    fn spawn_refresh(&mut self) {
        let Some(network) = self.network.clone() else {
            return;
        };

        self.refresh_in_progress = true;
        let tx = self.refresh_tx.clone();

        tokio::spawn(async move {
            let (statuses, block_info) = tokio::join!(
                crate::network::check_all_nodes_status_async(&network),
                crate::network::fetch_all_nodes_block_info(&network),
            );

            let results = RefreshResults {
                statuses,
                block_info,
            };

            let _ = tx.send(results).await;
        });
    }

    /// Calculate storage for all nodes.
    pub fn refresh_storage(&mut self) {
        if let Some(base_dir) = self.network_base_dir().map(String::from) {
            for node in &mut self.nodes {
                let storage = crate::network::calculate_node_storage(&base_dir, &node.name);
                node.storage = Some(storage);
            }
        }
    }

    /// Calculate storage for the currently selected node.
    pub fn refresh_selected_node_storage(&mut self) {
        if let Some(base_dir) = self.network_base_dir().map(String::from) {
            if let Some(node) = self.nodes.get_mut(self.selected_node_index) {
                let storage = crate::network::calculate_node_storage(&base_dir, &node.name);
                node.storage = Some(storage);
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert!(app.is_running());
        assert_eq!(app.current_view(), View::Nodes);
        assert_eq!(app.input_mode(), InputMode::Normal);
        assert_eq!(app.selected_node_index(), 0);
        assert!(app.nodes().is_empty());
    }

    #[test]
    fn test_app_quit() {
        let mut app = App::new();
        assert!(app.is_running());
        app.quit();
        assert!(!app.is_running());
    }

    #[test]
    fn test_view_navigation() {
        let mut app = App::new();
        assert_eq!(app.current_view(), View::Nodes);

        app.next_view();
        assert_eq!(app.current_view(), View::Details);

        app.next_view();
        assert_eq!(app.current_view(), View::Logs);

        app.next_view();
        assert_eq!(app.current_view(), View::Nodes);

        app.previous_view();
        assert_eq!(app.current_view(), View::Logs);
    }

    #[test]
    fn test_help_toggle() {
        let mut app = App::new();
        assert_eq!(app.input_mode(), InputMode::Normal);

        app.toggle_help();
        assert_eq!(app.input_mode(), InputMode::Help);

        app.toggle_help();
        assert_eq!(app.input_mode(), InputMode::Normal);
    }

    #[test]
    fn test_log_follow_toggle() {
        let mut app = App::new();
        assert!(app.log_viewer().follow());

        app.toggle_log_follow();
        assert!(!app.log_viewer().follow());

        app.toggle_log_follow();
        assert!(app.log_viewer().follow());
    }

    #[test]
    fn test_search_flow() {
        let mut app = App::new();
        assert_eq!(app.input_mode(), InputMode::Normal);

        app.start_search();
        assert_eq!(app.input_mode(), InputMode::Search);
        assert!(app.search_input().is_empty());

        app.add_search_char('h');
        app.add_search_char('e');
        app.add_search_char('l');
        app.add_search_char('l');
        app.add_search_char('o');

        assert_eq!(app.search_input(), "hello");

        app.remove_search_char();
        assert_eq!(app.search_input(), "hell");

        app.confirm_search();
        assert_eq!(app.input_mode(), InputMode::Normal);
    }

    #[test]
    fn test_status_message() {
        let mut app = App::new();
        assert!(app.status_message().is_none());

        app.set_status("Test message");
        assert_eq!(app.status_message(), Some("Test message"));

        app.clear_status();
        assert!(app.status_message().is_none());
    }

    #[test]
    fn test_confirmation_flow() {
        let mut app = App::new();
        assert_eq!(app.input_mode(), InputMode::Normal);
        assert!(app.pending_action().is_none());

        app.request_confirmation(PendingAction::RestartNode("test".to_string()));
        assert_eq!(app.input_mode(), InputMode::Confirm);
        assert!(app.pending_action().is_some());

        app.cancel_action();
        assert_eq!(app.input_mode(), InputMode::Normal);
        assert!(app.pending_action().is_none());
    }
}
