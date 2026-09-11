use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use super::node::JamNetworkNode;
use crate::utils::default_as_empty_vec;

/// The JAM chain of a network, and the nodes running it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Jamchain {
    pub(crate) id: String,
    pub(crate) chain_spec_path: PathBuf,
    #[serde(default, deserialize_with = "default_as_empty_vec")]
    pub(crate) nodes: Vec<Arc<JamNetworkNode>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawJamchain {
    #[serde(flatten)]
    pub(crate) inner: Jamchain,
    pub(crate) nodes: serde_json::Value,
}

impl Jamchain {
    pub(crate) fn new(id: impl Into<String>, chain_spec_path: PathBuf) -> Self {
        Self {
            id: id.into(),
            chain_spec_path,
            nodes: Default::default(),
        }
    }

    // Public API

    pub fn nodes(&self) -> Vec<&JamNetworkNode> {
        self.nodes.iter().map(|n| n.as_ref()).collect()
    }

    /// Get chain id
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn chain_spec_path(&self) -> &Path {
        self.chain_spec_path.as_path()
    }
}
