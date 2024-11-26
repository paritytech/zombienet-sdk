use std::path::PathBuf;

use anyhow::anyhow;
use async_trait::async_trait;
use serde::Serialize;

use super::node::NetworkNode;
use crate::{network::chain_upgrade::ChainUpgrade, shared::types::RuntimeUpgradeOptions};

#[derive(Debug, Serialize)]
pub struct Relaychain {
    pub(crate) chain: String,
    pub(crate) chain_id: String,
    pub(crate) chain_spec_path: PathBuf,
    pub(crate) nodes: Vec<NetworkNode>,
}

#[async_trait]
impl ChainUpgrade for Relaychain {
    async fn runtime_upgrade(&self, options: RuntimeUpgradeOptions) -> Result<(), anyhow::Error> {
        // check if the node is valid first
        let node = if let Some(node_name) = &options.node_name {
            if let Some(node) = self
                .nodes()
                .into_iter()
                .find(|node| node.name() == node_name)
            {
                node
            } else {
                return Err(anyhow!(
                    "Node: {} is not part of the set of nodes",
                    node_name
                ));
            }
        } else {
            // take the first node
            if let Some(node) = self.nodes().first() {
                node
            } else {
                return Err(anyhow!("chain doesn't have any node!"));
            }
        };

        self.perform_runtime_upgrade(node, options).await
    }
}

impl Relaychain {
    pub(crate) fn new(chain: String, chain_id: String, chain_spec_path: PathBuf) -> Self {
        Self {
            chain,
            chain_id,
            chain_spec_path,
            nodes: Default::default(),
        }
    }

    // Public API
    pub fn nodes(&self) -> Vec<&NetworkNode> {
        self.nodes.iter().collect()
    }

    /// Get chain name
    pub fn chain(&self) -> &str {
        &self.chain
    }
}
