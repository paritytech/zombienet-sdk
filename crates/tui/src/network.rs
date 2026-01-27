//! Network data extraction and adaptation for the TUI.

use std::path::PathBuf;

use futures::future::join_all;
use orchestrator::network::Network;
use support::fs::local::LocalFileSystem;

pub use crate::helpers::format_size;

/// Information about a node for display in the TUI.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Node name (e.g., "alice", "bob").
    pub name: String,
    /// Parachain ID.
    pub para_id: Option<u32>,
    /// Node type (relay validator, collator, etc.).
    pub node_type: NodeType,
    /// Current node status.
    pub status: NodeStatus,
    /// Storage usage information.
    pub storage: Option<StorageInfo>,
    /// Block height information.
    pub block_info: Option<BlockInfo>,
}

/// Block height information for a node.
#[derive(Debug, Clone, Default)]
pub struct BlockInfo {
    /// Best (head) block number.
    pub best: u64,
    /// Finalized block number.
    pub finalized: u64,
}

/// Configurable thresholds for storage level classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageThresholds {
    pub medium: u64,
    pub high: u64,
    pub critical: u64,
}

impl StorageThresholds {
    const GB: u64 = Self::MB * 1024;
    const MB: u64 = 1024 * 1024;

    pub fn new(medium: u64, high: u64, critical: u64) -> Self {
        Self {
            medium,
            high,
            critical,
        }
    }

    pub fn from_mb(medium_mb: u64, high_mb: u64, critical_mb: u64) -> Self {
        Self {
            medium: medium_mb * Self::MB,
            high: high_mb * Self::MB,
            critical: critical_mb * Self::MB,
        }
    }
}

impl Default for StorageThresholds {
    /// Default thresholds suitable for short-lived test networks.
    ///
    /// - Low: < 100 MB
    /// - Medium: 100 MB - 1 GB
    /// - High: 1 GB - 10 GB
    /// - Critical: > 10 GB
    fn default() -> Self {
        Self {
            medium: 100 * Self::MB,  // 100 MB
            high: Self::GB,          // 1 GB
            critical: 10 * Self::GB, // 10 GB
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StorageInfo {
    /// Total storage used by the node.
    pub total_bytes: u64,
    /// Storage used by the data directory.
    pub data_bytes: u64,
    /// Whether storage calculation is in progress.
    pub is_calculating: bool,
}

impl StorageInfo {
    pub fn total_formatted(&self) -> String {
        format_size(self.total_bytes)
    }

    /// Determine storage level based on thresholds.
    pub fn level_with_thresholds(&self, thresholds: &StorageThresholds) -> StorageLevel {
        if self.total_bytes >= thresholds.critical {
            StorageLevel::Critical
        } else if self.total_bytes >= thresholds.high {
            StorageLevel::High
        } else if self.total_bytes >= thresholds.medium {
            StorageLevel::Medium
        } else {
            StorageLevel::Low
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageLevel {
    /// Low storage usage (< 100 MB).
    Low,
    /// Medium storage usage (100 MB - 1 GB).
    Medium,
    /// High storage usage (1 GB - 10 GB).
    High,
    /// Critical storage usage (> 10 GB).
    Critical,
}

impl StorageLevel {
    pub fn color(&self) -> &'static str {
        match self {
            StorageLevel::Low => "green",
            StorageLevel::Medium => "yellow",
            StorageLevel::High => "red",
            StorageLevel::Critical => "red",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            StorageLevel::Low => "▁",
            StorageLevel::Medium => "▃",
            StorageLevel::High => "▅",
            StorageLevel::Critical => "█",
        }
    }
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

fn parse_node_status(is_running: bool) -> NodeStatus {
    if is_running {
        NodeStatus::Running
    } else {
        NodeStatus::Unknown
    }
}

/// Extract node information from a running network.
///
/// This checks each node's responsiveness by attempting to connect to its WebSocket endpoint.
pub async fn extract_nodes(network: &Network<LocalFileSystem>) -> Vec<NodeInfo> {
    let mut nodes = Vec::new();

    for node in network.relaychain().nodes() {
        let is_responsive = node.is_responsive().await;
        let status = parse_node_status(is_responsive);
        nodes.push(NodeInfo {
            name: node.name().to_string(),
            para_id: None,
            node_type: NodeType::Relay,
            status,
            storage: None,
            block_info: None,
        });
    }

    // Extract parachain collators.
    for para in network.parachains() {
        for collator in para.collators() {
            let is_responsive = collator.is_responsive().await;
            let status = parse_node_status(is_responsive);
            nodes.push(NodeInfo {
                name: collator.name().to_string(),
                para_id: Some(para.para_id()),
                node_type: NodeType::Collator,
                status,
                storage: None,
                block_info: None,
            });
        }
    }

    nodes
}

/// Check node status by verifying RPC connectivity.
pub async fn check_node_status_async(
    network: &Network<LocalFileSystem>,
    node_name: &str,
) -> NodeStatus {
    if let Ok(node) = network.get_node(node_name) {
        let is_responsive = node.is_responsive().await;
        parse_node_status(is_responsive)
    } else {
        NodeStatus::Unknown
    }
}

/// Check status of all nodes.
///
/// Returns a map of node name to status.
pub async fn check_all_nodes_status_async(
    network: &Network<LocalFileSystem>,
) -> std::collections::HashMap<String, NodeStatus> {
    let mut status_map = std::collections::HashMap::new();

    let mut node_names: Vec<String> = network
        .relaychain()
        .nodes()
        .iter()
        .map(|n| n.name().to_string())
        .collect();

    for para in network.parachains() {
        for collator in para.collators() {
            node_names.push(collator.name().to_string());
        }
    }

    let futures: Vec<_> = node_names
        .iter()
        .map(|name| async {
            let status = check_node_status_async(network, name).await;
            (name.clone(), status)
        })
        .collect();

    let results = join_all(futures).await;

    for (name, status) in results {
        status_map.insert(name, status);
    }

    status_map
}

/// Prometheus metric names for block heights.
const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
const FINALIZED_BLOCK_METRIC: &str = "block_height{status=\"finalized\"}";

/// Fetch block info for a single node.
///
/// Returns `None` if the node is not running or metrics are unavailable.
pub async fn fetch_node_block_info(
    network: &Network<LocalFileSystem>,
    node_name: &str,
) -> Option<BlockInfo> {
    let node = network.get_node(node_name).ok()?;

    if !node.is_responsive().await {
        return None;
    }

    let best = node.reports(BEST_BLOCK_METRIC).await.ok()?;
    let finalized = node.reports(FINALIZED_BLOCK_METRIC).await.ok()?;

    Some(BlockInfo {
        best: best as u64,
        finalized: finalized as u64,
    })
}

/// Fetch block info for all nodes in parallel.
///
/// Returns a map of node name to block info.
pub async fn fetch_all_nodes_block_info(
    network: &Network<LocalFileSystem>,
) -> std::collections::HashMap<String, BlockInfo> {
    let mut node_names: Vec<String> = network
        .relaychain()
        .nodes()
        .iter()
        .map(|n| n.name().to_string())
        .collect();

    for para in network.parachains() {
        for collator in para.collators() {
            node_names.push(collator.name().to_string());
        }
    }

    let futures: Vec<_> = node_names
        .iter()
        .map(|name| async {
            let block_info = fetch_node_block_info(network, name).await;
            (name.clone(), block_info)
        })
        .collect();

    let results = join_all(futures).await;

    results
        .into_iter()
        .filter_map(|(name, info)| info.map(|i| (name, i)))
        .collect()
}

/// Calculate storage for a single node given the base directory.
pub fn calculate_node_storage(base_dir: &str, node_name: &str) -> StorageInfo {
    let node_dir = PathBuf::from(base_dir).join(node_name);
    let data_dir = node_dir.join("data");

    let total_bytes = calculate_dir_size(&node_dir).unwrap_or(0);
    let data_bytes = calculate_dir_size(&data_dir).unwrap_or(0);

    StorageInfo {
        total_bytes,
        data_bytes,
        is_calculating: false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_storage_info_formatted() {
        let storage = StorageInfo {
            total_bytes: 1024 * 1024 * 512, // 512 MB
            data_bytes: 1024 * 1024 * 256,  // 256 MB
            is_calculating: false,
        };
        assert_eq!(storage.total_formatted(), "512.00 MB");
    }

    #[test]
    fn test_storage_level() {
        const MB: u64 = 1024 * 1024;
        const GB: u64 = MB * 1024;

        // Low: < 100 MB
        let low = StorageInfo {
            total_bytes: 50 * MB,
            data_bytes: 0,
            is_calculating: false,
        };
        assert_eq!(
            low.level_with_thresholds(&StorageThresholds::default()),
            StorageLevel::Low
        );

        // Medium: 100 MB - 1 GB
        let medium = StorageInfo {
            total_bytes: 500 * MB,
            data_bytes: 0,
            is_calculating: false,
        };
        assert_eq!(
            medium.level_with_thresholds(&StorageThresholds::default()),
            StorageLevel::Medium
        );

        // High: 1 GB - 10 GB
        let high = StorageInfo {
            total_bytes: 5 * GB,
            data_bytes: 0,
            is_calculating: false,
        };
        assert_eq!(
            high.level_with_thresholds(&StorageThresholds::default()),
            StorageLevel::High
        );

        // Critical: > 10 GB
        let critical = StorageInfo {
            total_bytes: 15 * GB,
            data_bytes: 0,
            is_calculating: false,
        };
        assert_eq!(
            critical.level_with_thresholds(&StorageThresholds::default()),
            StorageLevel::Critical
        );
    }

    #[test]
    fn test_storage_level_icons() {
        assert_eq!(StorageLevel::Low.icon(), "▁");
        assert_eq!(StorageLevel::Medium.icon(), "▃");
        assert_eq!(StorageLevel::High.icon(), "▅");
        assert_eq!(StorageLevel::Critical.icon(), "█");
    }

    #[test]
    fn test_storage_thresholds_custom() {
        const GB: u64 = 1024 * 1024 * 1024;

        let thresholds = StorageThresholds::from_mb(1024, 10240, 102400); // 1GB, 10GB, 100GB

        let storage = StorageInfo {
            total_bytes: 15 * GB, // 15 GB would be "Critical" with defaults.
            data_bytes: 0,
            is_calculating: false,
        };

        // With defaults, 15 GB is Critical.
        assert_eq!(
            storage.level_with_thresholds(&StorageThresholds::default()),
            StorageLevel::Critical
        );

        // With custom thresholds, 15 GB (between 10GB and 100GB) is High.
        assert_eq!(
            storage.level_with_thresholds(&thresholds),
            StorageLevel::High
        );
    }

    #[test]
    fn test_storage_thresholds_from_mb() {
        let thresholds = StorageThresholds::from_mb(500, 2000, 5000);
        const MB: u64 = 1024 * 1024;

        assert_eq!(thresholds.medium, 500 * MB);
        assert_eq!(thresholds.high, 2000 * MB);
        assert_eq!(thresholds.critical, 5000 * MB);
    }
}
