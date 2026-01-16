//! Network data extraction and adaptation for the TUI.

use std::path::PathBuf;

use orchestrator::network::Network;
use support::fs::local::LocalFileSystem;

/// Information about a node for display in the TUI.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Node name (e.g., "alice", "bob").
    pub name: String,
    /// WebSocket RPC URI.
    pub ws_uri: String,
    /// libp2p multiaddress.
    pub multiaddr: String,
    /// Parachain ID (if this is a collator).
    pub para_id: Option<u32>,
    /// Node type (relay validator, collator, etc.).
    pub node_type: NodeType,
    /// Current node status.
    pub status: NodeStatus,
}

/// The type of node in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// Relay chain node.
    Relay,
    /// Parachain collator node.
    Collator,
}

impl NodeType {
    /// Get a short display string for the node type.
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Relay => "relay",
            NodeType::Collator => "collator",
        }
    }

    /// Get an icon/symbol for the node type.
    pub fn icon(&self) -> &'static str {
        match self {
            NodeType::Relay => "R",
            NodeType::Collator => "C",
        }
    }
}

/// Current status of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeStatus {
    /// Node is running normally.
    #[default]
    Running,
    /// Node is paused (SIGSTOP).
    Paused,
    /// Node status is unknown.
    Unknown,
}

impl NodeStatus {
    /// Get a display string for the status.
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeStatus::Running => "running",
            NodeStatus::Paused => "paused",
            NodeStatus::Unknown => "unknown",
        }
    }

    /// Get a status icon/symbol.
    pub fn icon(&self) -> &'static str {
        match self {
            NodeStatus::Running => "●",
            NodeStatus::Paused => "◐",
            NodeStatus::Unknown => "○",
        }
    }
}

/// Extract node information from a running network.
pub fn extract_nodes(network: &Network<LocalFileSystem>) -> Vec<NodeInfo> {
    let mut nodes = Vec::new();

    // Extract relay chain nodes.
    for node in network.relaychain().nodes() {
        nodes.push(NodeInfo {
            name: node.name().to_string(),
            ws_uri: node.ws_uri().to_string(),
            multiaddr: node.multiaddr().to_string(),
            para_id: None,
            node_type: NodeType::Relay,
            status: NodeStatus::Running, // Assume running initially.
        });
    }

    // Extract parachain collators.
    for para in network.parachains() {
        for collator in para.collators() {
            nodes.push(NodeInfo {
                name: collator.name().to_string(),
                ws_uri: collator.ws_uri().to_string(),
                multiaddr: collator.multiaddr().to_string(),
                para_id: Some(para.para_id()),
                node_type: NodeType::Collator,
                status: NodeStatus::Running, // Assume running initially.
            });
        }
    }

    nodes
}

/// Derive the log path for a node given the network base directory.
pub fn derive_log_path(base_dir: &str, node_name: &str) -> PathBuf {
    PathBuf::from(base_dir)
        .join(node_name)
        .join(format!("{}.log", node_name))
}

/// Derive the data directory path for a node.
pub fn derive_data_dir(base_dir: &str, node_name: &str) -> PathBuf {
    PathBuf::from(base_dir).join(node_name).join("data")
}

/// Calculate the size of a directory in bytes.
pub fn calculate_dir_size(path: &PathBuf) -> std::io::Result<u64> {
    let mut total_size = 0u64;

    if path.is_dir() {
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                total_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }

    Ok(total_size)
}

/// Format a byte size as a human-readable string.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 2), "2.00 GB");
    }

    #[test]
    fn test_node_type_as_str() {
        assert_eq!(NodeType::Relay.as_str(), "relay");
        assert_eq!(NodeType::Collator.as_str(), "collator");
    }

    #[test]
    fn test_node_type_icon() {
        assert_eq!(NodeType::Relay.icon(), "R");
        assert_eq!(NodeType::Collator.icon(), "C");
    }

    #[test]
    fn test_derive_log_path() {
        let path = derive_log_path("/tmp/zombie-123", "alice");
        assert_eq!(path, PathBuf::from("/tmp/zombie-123/alice/alice.log"));
    }

    #[test]
    fn test_derive_data_dir() {
        let path = derive_data_dir("/tmp/zombie-123", "alice");
        assert_eq!(path, PathBuf::from("/tmp/zombie-123/alice/data"));
    }

    #[test]
    fn test_node_status_as_str() {
        assert_eq!(NodeStatus::Running.as_str(), "running");
        assert_eq!(NodeStatus::Paused.as_str(), "paused");
        assert_eq!(NodeStatus::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_node_status_icon() {
        assert_eq!(NodeStatus::Running.icon(), "●");
        assert_eq!(NodeStatus::Paused.icon(), "◐");
        assert_eq!(NodeStatus::Unknown.icon(), "○");
    }
}
