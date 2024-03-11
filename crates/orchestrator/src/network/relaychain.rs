use std::path::PathBuf;

use super::node::NetworkNode;

#[derive(Debug)]
pub struct Relaychain {
    pub(crate) chain: String,
    pub(crate) chain_id: String,
    pub(crate) chain_spec_path: PathBuf,
    pub(crate) nodes: Vec<NetworkNode>,
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
    pub fn chain(&self) -> &str {
        &self.chain
    }

    pub fn nodes(&self) -> Vec<&NetworkNode> {
        self.nodes.iter().collect()
    }
}
