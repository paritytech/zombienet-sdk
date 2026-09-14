use anyhow::anyhow;
use configuration::types::ParaId;
use subxt::{backend::rpc::RpcParams, OnlineClient, PolkadotConfig};
use tracing::debug;

use crate::network::node::NetworkNode;

/// Check if the parachain id is registered
pub async fn is_registered(
    node: &NetworkNode,
    para_id: ParaId
) -> Result<bool, anyhow::Error> {
    debug!("Checking if para_id: {para_id} is registered");

    let paras = paras(node).await?;

    let is_registered = paras.iter().any(|p| *p == para_id);

    debug!("Checking id para_id is registered: {is_registered}");
    Ok(is_registered)
}

/// Get registered paras
pub async fn paras(node: &NetworkNode) -> Result<Vec<ParaId>, anyhow::Error> {
    let api: OnlineClient<PolkadotConfig> = node.wait_client().await?;
    //api.storage().
    let paras_addr = subxt::dynamic::storage("paras", "parachains", vec![]);


    let chunk = api.storage().at_latest().await?.fetch(&paras_addr).await?.ok_or(anyhow!("Paras_parachains should be present. qed"))?;
    let paras: Vec<u32> = chunk.as_type()?;
    // to_value() {
    //     Ok(v) => v,
    //     Err(e) => return Some(Err(e.into())),
    // };

    //  {
    //     Ok(Some(v)) => v,
    //     Ok(None) => {
    //         // The storage `system::lastRuntimeUpgrade` should always exist.
    //         // <https://github.com/paritytech/polkadot-sdk/blob/master/substrate/frame/system/src/lib.rs#L958>
    //         unreachable!("The storage item `system::lastRuntimeUpgrade` should always exist")
    //     }
    //     Err(e) => return Some(Err(e)),
    // };


    let rpc = node.rpc().await?;
    let paras: Vec<ParaId> = rpc.request("Paras_parachains", RpcParams::default()).await?;

    Ok(paras)
}
