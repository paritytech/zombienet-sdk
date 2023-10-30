use std::{
    path::{Path, PathBuf},
};

use super::node::NetworkNode;

#[derive(Debug)]
pub struct Parachain {
    pub(crate) chain: Option<String>,
    pub(crate) para_id: u32,
    pub(crate) chain_id: Option<String>,
    pub(crate) chain_spec_path: Option<PathBuf>,
    pub(crate) collators: Vec<NetworkNode>,
}

impl Parachain {
    pub(crate) fn new(para_id: u32) -> Self {
        Self {
            chain: None,
            para_id,
            chain_id: None,
            chain_spec_path: None,
            collators: Default::default(),
        }
    }

    pub(crate) fn with_chain_spec(
        para_id: u32,
        chain_id: impl Into<String>,
        chain_spec_path: impl AsRef<Path>,
    ) -> Self {
        Self {
            para_id,
            chain: None,
            chain_id: Some(chain_id.into()),
            chain_spec_path: Some(chain_spec_path.as_ref().into()),
            collators: Default::default(),
        }
    }
}
