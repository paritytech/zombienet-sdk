use std::str::FromStr;

use anyhow::anyhow;
use async_trait::async_trait;
use subxt_signer::{sr25519::Keypair, SecretUri};

use super::node::NetworkNode;
use crate::{shared::types::RuntimeUpgradeOptions, tx_helper};

#[async_trait]
pub trait ChainUpgrade {
    /// Perform a runtime upgrade (with sudo)
    ///
    /// This call 'System.set_code_without_checks' wrapped in
    /// 'Sudo.sudo_unchecked_weight'
    async fn runtime_upgrade(&self, options: RuntimeUpgradeOptions) -> Result<(), anyhow::Error>;

    /// Perform a runtime upgrade (with sudo), inner call with the node pass as arg.
    ///
    /// This call 'System.set_code_without_checks' wrapped in
    /// 'Sudo.sudo_unchecked_weight'
    async fn perform_runtime_upgrade(
        &self,
        node: &NetworkNode,
        options: RuntimeUpgradeOptions,
    ) -> Result<(), anyhow::Error> {
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
