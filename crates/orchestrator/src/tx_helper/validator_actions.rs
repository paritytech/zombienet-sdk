use std::str::FromStr;

use subxt::{dynamic::Value, OnlineClient, SubstrateConfig};
use subxt_signer::{sr25519::Keypair, SecretUri};

#[subxt::subxt(runtime_metadata_path = "src/metadata.scale")]
pub mod substrate{}

pub async fn register(
    validator_ids: Vec<String>,
    node_ws_url: &str,
) -> Result<(), anyhow::Error> {
    println!("Registering validators: {:?}", validator_ids);
    let uri = SecretUri::from_str("//Alice")?;
    let sudo = Keypair::from_uri(&uri)?;

    println!("pse");
    let api = OnlineClient::<SubstrateConfig>::from_url(node_ws_url).await?;
    println!("pse connected");

    let register_call = substrate::ValidatorManager::register_validators(
        vec![Value::unnamed_composite(vec![Value::from_bytes(validator_ids.first().unwrap().as_bytes())])],
    );

    let sudo_call = substrate::Sudo::sudo(register_call.into_value());

    println!("pse1");
    // TODO: uncomment below and fix the sign and submit (and follow afterwards until
    // finalized block) to register the parachain
    let result = api
        .tx()
        .sign_and_submit_then_watch_default(&sudo_call, &sudo)
        .await?;

    println!("result: {:#?}", result);
    let result = result.wait_for_in_block().await?;
    println!("In block: {:#?}", result.block_hash());
    Ok(())
}