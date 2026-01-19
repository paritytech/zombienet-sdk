//! Application state and core logic for the TUI.

use std::path::PathBuf;

use anyhow::Result;
use orchestrator::network::Network;
use support::fs::local::LocalFileSystem;
use zombienet_sdk::AttachToLive;

use crate::{
    logs::LogViewer,
    network::{NodeInfo, NodeStatus},
    watcher::{FileWatcher, WatchEvent},
};

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
    /// Confirmation dialog is active.
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
    network: Option<Network<LocalFileSystem>>,
    /// Path to the zombie.json file.
    zombie_json_path: Option<PathBuf>,
    /// Current view/panel focus.
    pub current_view: View,
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
}

/// Actions that require user confirmation.
#[derive(Debug, Clone)]
pub enum PendingAction {
    RestartNode(String),
    ShutdownNetwork,
}

impl App {
    /// Create a new App instance.
    pub fn new() -> Self {
        let file_watcher = FileWatcher::new().ok();

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

    /// Set the zombie.json path for attachment.
    pub fn set_zombie_json_path(&mut self, path: PathBuf) {
        self.zombie_json_path = Some(path);
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
        self.nodes = crate::network::extract_nodes(&network);
        self.network = Some(network);

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
        if let Some(network) = &self.network {
            if let Some(node_info) = self.nodes.get(self.selected_node_index) {
                let name = node_info.name.clone();
                let node = network.get_node(&name)?;
                node.pause().await?;
                // Update status.
                if let Some(node_info) = self.nodes.get_mut(self.selected_node_index) {
                    node_info.status = NodeStatus::Paused;
                }
                self.set_status(format!("Paused node: {}", name));
            }
        }
        Ok(())
    }

    /// Resume the currently selected node.
    pub async fn resume_selected_node(&mut self) -> Result<()> {
        if let Some(network) = &self.network {
            if let Some(node_info) = self.nodes.get(self.selected_node_index) {
                let name = node_info.name.clone();
                let node = network.get_node(&name)?;
                node.resume().await?;
                // Update status.
                if let Some(node_info) = self.nodes.get_mut(self.selected_node_index) {
                    node_info.status = NodeStatus::Running;
                }
                self.set_status(format!("Resumed node: {}", name));
            }
        }
        Ok(())
    }

    /// Restart a specific node.
    async fn restart_node(&mut self, name: &str) -> Result<()> {
        if let Some(network) = &self.network {
            let node = network.get_node(name)?;
            node.restart(None).await?;
            self.set_status(format!("Restarted node: {}", name));
            self.refresh_nodes();
        }
        Ok(())
    }

    /// Shutdown the entire network.
    async fn shutdown_network(&mut self) -> Result<()> {
        if let Some(network) = self.network.take() {
            network.destroy().await?;
            self.set_status("Network shutdown complete");
            self.nodes.clear();
            self.quit();
        }
        Ok(())
    }

    /// Refresh the node list from the network.
    pub fn refresh_nodes(&mut self) {
        if let Some(network) = &self.network {
            self.nodes = crate::network::extract_nodes(network);
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
    pub async fn tick(&mut self) {
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
