use subxt::{dynamic::Value, tx::TxStatus, OnlineClient, SubstrateConfig};
use subxt_signer::sr25519::Keypair;
use tracing::{debug, info};

use crate::network::node::NetworkNode;

pub async fn upgrade(
    node: &NetworkNode,
    wasm_data: &[u8],
    sudo: &Keypair,
) -> Result<(), anyhow::Error> {
    debug!(
        "Upgrading runtime, using node: {} with endpoting {}",
        node.name, node.ws_uri
    );
    let api: OnlineClient<SubstrateConfig> = node.wait_client().await?;

    let upgrade = subxt::dynamic::tx(
        "System",
        "set_code_without_checks",
        vec![Value::from_bytes(wasm_data)],
    );

    let sudo_call = subxt::dynamic::tx(
        "Sudo",
        "sudo_unchecked_weight",
        vec![
            upgrade.into_value(),
            Value::named_composite([
                ("ref_time", Value::primitive(1.into())),
                ("proof_size", Value::primitive(1.into())),
            ]),
        ],
    );

    let mut tx = api
        .tx()
        .sign_and_submit_then_watch_default(&sudo_call, sudo)
        .await?;

    // Below we use the low level API to replicate the `wait_for_in_block` behaviour
    // which was removed in subxt 0.33.0. See https://github.com/paritytech/subxt/pull/1237.
    while let Some(status) = tx.next().await {
        let status = status?;
        match &status {
            TxStatus::InBestBlock(tx_in_block) | TxStatus::InFinalizedBlock(tx_in_block) => {
                let _result = tx_in_block.wait_for_success().await?;
                let block_status = if status.as_finalized().is_some() {
                    "Finalized"
                } else {
                    "Best"
                };
                info!(
                    "[{}] In block: {:#?}",
                    block_status,
                    tx_in_block.block_hash()
                );
            },
            TxStatus::Error { message }
            | TxStatus::Invalid { message }
            | TxStatus::Dropped { message } => {
                return Err(anyhow::format_err!("Error submitting tx: {message}"));
            },
            _ => continue,
        }
    }

    Ok(())
}
