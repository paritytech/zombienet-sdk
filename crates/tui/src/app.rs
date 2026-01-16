//! Application state and core logic for the TUI.

use std::path::PathBuf;

use anyhow::Result;
use orchestrator::network::Network;
use support::fs::local::LocalFileSystem;
use zombienet_sdk::AttachToLive;

use crate::network::NodeInfo;

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
    /// Log lines for the currently selected node.
    log_lines: Vec<String>,
    /// Whether to auto-scroll logs (follow mode).
    log_follow: bool,
    /// Scroll position in the log viewer.
    log_scroll: usize,
    /// Status message to display.
    status_message: Option<String>,
    /// Pending confirmation action.
    pending_action: Option<PendingAction>,
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
        Self {
            running: true,
            network: None,
            zombie_json_path: None,
            current_view: View::Nodes,
            input_mode: InputMode::Normal,
            selected_node_index: 0,
            nodes: Vec::new(),
            log_lines: Vec::new(),
            log_follow: true,
            log_scroll: 0,
            status_message: None,
            pending_action: None,
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

    /// Get the log lines for the current node.
    pub fn log_lines(&self) -> &[String] {
        &self.log_lines
    }

    /// Get the current log scroll position.
    pub fn log_scroll(&self) -> usize {
        self.log_scroll
    }

    /// Check if log follow mode is enabled.
    pub fn log_follow(&self) -> bool {
        self.log_follow
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
        self.log_follow = !self.log_follow;
    }

    /// Scroll logs up.
    pub fn scroll_logs_up(&mut self, amount: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(amount);
        self.log_follow = false;
    }

    /// Scroll logs down.
    pub fn scroll_logs_down(&mut self, amount: usize) {
        let max_scroll = self.log_lines.len().saturating_sub(1);
        self.log_scroll = (self.log_scroll + amount).min(max_scroll);
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
                }
                PendingAction::ShutdownNetwork => {
                    self.shutdown_network().await?;
                }
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
        if let (Some(network), Some(node_info)) = (&self.network, self.selected_node()) {
            let name = node_info.name.clone();
            let node = network.get_node(&name)?;
            node.pause().await?;
            self.set_status(format!("Paused node: {}", name));
            self.refresh_nodes();
        }
        Ok(())
    }

    /// Resume the currently selected node.
    pub async fn resume_selected_node(&mut self) -> Result<()> {
        if let (Some(network), Some(node_info)) = (&self.network, self.selected_node()) {
            let name = node_info.name.clone();
            let node = network.get_node(&name)?;
            node.resume().await?;
            self.set_status(format!("Resumed node: {}", name));
            self.refresh_nodes();
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
        if let (Some(network), Some(node_info)) = (&self.network, self.selected_node()) {
            let node = network.get_node(&node_info.name)?;
            let logs = node.logs().await?;
            self.log_lines = logs.lines().map(String::from).collect();

            if self.log_follow {
                self.log_scroll = self.log_lines.len().saturating_sub(1);
            }
        }
        Ok(())
    }

    /// Periodic tick for async updates.
    pub async fn tick(&mut self) {
        if self.log_follow && self.current_view == View::Logs {
            let _ = self.load_selected_node_logs().await;
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
        assert!(app.log_follow());

        app.toggle_log_follow();
        assert!(!app.log_follow());

        app.toggle_log_follow();
        assert!(app.log_follow());
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
