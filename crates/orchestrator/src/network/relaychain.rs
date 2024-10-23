use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;

use super::node::NetworkNode;
use crate::network::chain_upgrade::ChainUpgrade;

#[derive(Debug, Serialize)]
pub struct Relaychain {
    pub(crate) chain: String,
    pub(crate) chain_id: String,
    pub(crate) chain_spec_path: PathBuf,
    pub(crate) nodes: Vec<NetworkNode>,
}

#[async_trait]
impl ChainUpgrade for Relaychain {
    fn nodes(&self) -> Vec<&NetworkNode> {
        self.nodes.iter().collect()
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

    /// Get chain name
    pub fn chain(&self) -> &str {
        &self.chain
    }

}
