use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputError {
    #[error("timeout_secs must be between 1 and 120")]
    InvalidTimeout,
    #[error("blocks must be between 1 and 100")]
    InvalidBlocks,
    #[error("log_lines must be between 1 and 1000")]
    InvalidLogLines,
}

fn default_log_lines() -> usize {
    200
}

fn default_timeout_secs() -> u64 {
    10
}

fn default_blocks() -> usize {
    2
}

fn validate_timeout_secs(timeout_secs: u64) -> Result<(), InputError> {
    if !(1..=120).contains(&timeout_secs) {
        return Err(InputError::InvalidTimeout);
    }

    Ok(())
}

fn validate_log_lines(log_lines: usize) -> Result<(), InputError> {
    if !(1..=1000).contains(&log_lines) {
        return Err(InputError::InvalidLogLines);
    }

    Ok(())
}

fn validate_blocks(blocks: usize) -> Result<(), InputError> {
    if !(1..=100).contains(&blocks) {
        return Err(InputError::InvalidBlocks);
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiagnoseRunInput {
    #[schemars(description = "Path to zombie.json for the run to diagnose")]
    pub zombie_json_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigInput {
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListNodesInput {
    pub zombie_json_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeInput {
    pub zombie_json_path: PathBuf,
    pub node_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MetricInput {
    pub zombie_json_path: PathBuf,
    pub node_name: String,
    pub metric_name: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl MetricInput {
    pub fn validate(&self) -> Result<(), InputError> {
        validate_timeout_secs(self.timeout_secs)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BlockProductionInput {
    pub zombie_json_path: PathBuf,
    pub node_name: String,
    #[serde(default = "default_blocks")]
    pub blocks: usize,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl BlockProductionInput {
    pub fn validate(&self) -> Result<(), InputError> {
        validate_blocks(self.blocks)?;
        validate_timeout_secs(self.timeout_secs)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeLogsInput {
    pub zombie_json_path: PathBuf,
    pub node_name: String,
    #[serde(default = "default_log_lines")]
    pub lines: usize,
}

impl NodeLogsInput {
    pub fn validate(&self) -> Result<(), InputError> {
        validate_log_lines(self.lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_input_carries_zombie_json_path() {
        let input = DiagnoseRunInput {
            zombie_json_path: PathBuf::from("zombie.json"),
        };

        assert_eq!(input.zombie_json_path, PathBuf::from("zombie.json"));
    }

    #[test]
    fn metric_input_rejects_invalid_timeout() {
        let input = MetricInput {
            zombie_json_path: PathBuf::from("zombie.json"),
            node_name: "alice".to_string(),
            metric_name: "process_start_time_seconds".to_string(),
            timeout_secs: 121,
        };

        assert_eq!(input.validate(), Err(InputError::InvalidTimeout));
    }

    #[test]
    fn metric_input_defaults_timeout_to_ten_seconds() {
        let input: MetricInput = serde_json::from_value(serde_json::json!({
            "zombie_json_path": "zombie.json",
            "node_name": "alice",
            "metric_name": "process_start_time_seconds"
        }))
        .expect("metric input should deserialize with a default timeout");

        assert_eq!(input.timeout_secs, 10);
        assert_eq!(input.validate(), Ok(()));
    }

    #[test]
    fn node_logs_input_rejects_zero_lines() {
        let input = NodeLogsInput {
            zombie_json_path: PathBuf::from("zombie.json"),
            node_name: "alice".to_string(),
            lines: 0,
        };

        assert_eq!(input.validate(), Err(InputError::InvalidLogLines));
    }

    #[test]
    fn block_production_input_rejects_invalid_blocks() {
        let input = BlockProductionInput {
            zombie_json_path: PathBuf::from("zombie.json"),
            node_name: "alice".to_string(),
            blocks: 0,
            timeout_secs: 10,
        };

        assert_eq!(input.validate(), Err(InputError::InvalidBlocks));
    }

    #[test]
    fn block_production_input_rejects_invalid_timeout() {
        let input = BlockProductionInput {
            zombie_json_path: PathBuf::from("zombie.json"),
            node_name: "alice".to_string(),
            blocks: 2,
            timeout_secs: 121,
        };

        assert_eq!(input.validate(), Err(InputError::InvalidTimeout));
    }

    #[test]
    fn default_blocks_is_two() {
        assert_eq!(default_blocks(), 2);
    }

    #[test]
    fn list_nodes_input_carries_zombie_json_and_optional_provider() {
        let input = ListNodesInput {
            zombie_json_path: PathBuf::from("zombie.json"),
        };

        assert_eq!(input.zombie_json_path, PathBuf::from("zombie.json"));
    }
}
