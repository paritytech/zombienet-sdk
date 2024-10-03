use std::{path::PathBuf, str::FromStr};

use anyhow::anyhow;
use serde::Serialize;
use subxt_signer::{sr25519::Keypair, SecretUri};

use super::node::NetworkNode;
use crate::{shared::types::RuntimeUpgradeOptions, tx_helper};

#[derive(Debug, Serialize)]
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

    /// Perform a runtime upgrade (with sudo)
    ///
    /// This call 'System.set_code_without_checks' wrapped in
    /// 'Sudo.sudo_unchecked_weight'
    pub async fn runtime_upgrade(
        &self,
        options: RuntimeUpgradeOptions,
    ) -> Result<(), anyhow::Error> {
        // check if the node is valid first
        let node = if let Some(node_name) = options.node_name {
            if let Some(node) = self
                .nodes()
                .into_iter()
                .find(|node| node.name() == node_name)
            {
                node
            } else {
                return Err(anyhow!("Node: {} is not part of the relaychain", node_name));
            }
        } else {
            // take the first node
            if let Some(node) = self.nodes().first() {
                node
            } else {
                return Err(anyhow!("Relaychain doesn't have any node!"));
            }
        };

        let sudo = if let Some(possible_seed) = options.seed {
            Keypair::from_secret_key(possible_seed)
                .map_err(|_| anyhow!("seed should return a Keypair"))?
        } else {
            let uri = SecretUri::from_str("//Alice")?;
            Keypair::from_uri(&uri).map_err(|_| anyhow!("'//Alice' should return a Keypair"))?
        };

        let wasm_data = options.wasm.get_asset().await?;

        tx_helper::runtime_upgrade::upgrade(node, &wasm_data, &sudo).await?;

        Ok(())
    }
}
