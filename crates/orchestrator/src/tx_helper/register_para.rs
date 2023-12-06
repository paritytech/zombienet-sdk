use std::str::FromStr;

use subxt::{dynamic::Value, OnlineClient, SubstrateConfig};
use subxt_signer::{sr25519::Keypair, SecretUri};
use support::fs::FileSystem;

use crate::{shared::types::RegisterParachainOptions, ScopedFilesystem};
use tracing::{debug, info, trace};


pub async fn register(
    options: RegisterParachainOptions,
    scoped_fs: &ScopedFilesystem<'_, impl FileSystem>,
) -> Result<(), anyhow::Error> {
    debug!("Registering parachain: {:?}", options);
    // get the seed
    let sudo: Keypair;
    if let Some(possible_seed) = options.seed {
        sudo = Keypair::from_seed(possible_seed).expect("seed should return a Keypair.");
    } else {
        let uri = SecretUri::from_str("//Alice")?;
        sudo = Keypair::from_uri(&uri)?;
    }

    let genesis_state = scoped_fs
        .read_to_string(options.state_path)
        .await
        .expect("State Path should be ok by this point.");
    let wasm_data = scoped_fs
        .read_to_string(options.wasm_path)
        .await
        .expect("Wasm Path should be ok by this point.");

    let api = OnlineClient::<SubstrateConfig>::from_url(options.node_ws_url).await?;

    let schedule_para = subxt::dynamic::tx(
        "ParasSudoWrapper",
        "sudo_schedule_para_initialize",
        vec![
            Value::primitive(options.id.into()),
            Value::named_composite([
                (
                    "genesis_head",
                    Value::from_bytes(hex::decode(&genesis_state[2..])?),
                ),
                (
                    "validation_code",
                    Value::from_bytes(hex::decode(&wasm_data[2..])?),
                ),
                ("para_kind", Value::bool(options.onboard_as_para)),
            ]),
        ],
    );

    let sudo_call = subxt::dynamic::tx("Sudo", "sudo", vec![schedule_para.into_value()]);

    // TODO: uncomment below and fix the sign and submit (and follow afterwards until
    // finalized block) to register the parachain
    let result = api
        .tx()
        .sign_and_submit_then_watch_default(&sudo_call, &sudo)
        .await?;

    let result = result.wait_for_in_block().await?;
    debug!("In block: {:#?}", result.block_hash());
    Ok(())
}