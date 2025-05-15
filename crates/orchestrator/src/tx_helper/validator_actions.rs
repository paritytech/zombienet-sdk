use std::str::FromStr;

use subxt::{dynamic::Value, OnlineClient, SubstrateConfig};
use subxt_signer::{sr25519::Keypair, SecretUri};
use tracing::{debug, info, trace};

pub async fn register(validator_ids: Vec<String>, node_ws_url: &str) -> Result<(), anyhow::Error> {
    debug!("Registering validators: {:?}", validator_ids);
    // get the seed
    // let sudo: Keypair;
    // if let Some(possible_seed) = options.seed {
    //     sudo = Keypair::from_seed(possible_seed).expect("seed should return a Keypair.");
    // } else {
    let uri = SecretUri::from_str("//Alice")?;
    let sudo = Keypair::from_uri(&uri)?;
    // }

    let api: OnlineClient<SubstrateConfig> = get_client_from_url(&options.node_ws_url).await?;

    let register_call = subxt::dynamic::tx(
        "ValidatorManager",
        "register_validators",
        vec![Value::unnamed_composite(vec![Value::from_bytes(
            validator_ids.first().unwrap().as_bytes(),
        )])],
    );

    let sudo_call = subxt::dynamic::tx("Sudo", "sudo", vec![register_call.into_value()]);

    // TODO: uncomment below and fix the sign and submit (and follow afterwards until
    // finalized block) to register the parachain
    let result = api
        .tx()
        .sign_and_submit_then_watch_default(&sudo_call, &sudo)
        .await?;

    debug!("result: {:#?}", result);
    let result = result.wait_for_in_block().await?;
    debug!("In block: {:#?}", result.block_hash());
    Ok(())
}
