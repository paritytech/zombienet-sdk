use std::{path::{Path, PathBuf}, fs};
use super::node::NetworkNode;

use crate::shared::types::{RegisterParachainOptions, ParachainGenesisArgs};
use subxt::{OnlineClient, SubstrateConfig, config::{polkadot, substrate}};
use subxt_signer::sr25519::dev;

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

    pub async fn register(&mut self, options: RegisterParachainOptions) {
        println!("Registering parachain: {:?}", options);

        // let wasm_data = fs::read_to_string(options.wasm_path.into()).unwrap();
        // let genesis_state = fs::read_to_string(options.state_path.into()).expect("Should have been able to read the file");

        // let parachain_genesis_args: ParachainGenesisArgs = ParachainGenesisArgs {
        //     genesis_head: genesis_state,
        //     validation_code: wasm_data,
        //     parachain: options.onboard_as_para,
        //   };

        // let api = OnlineClient::<SubstrateConfig>::from_url(options.node_ws_url).await.unwrap();
        

        
        // TS code
        // await api.tx.sudo
        // .sudo(api.tx.parasSudoWrapper.sudoScheduleParaInitialize(id, genesis))
        // .signAndSend(sudo, { nonce: nonce, era: 0 }, (result) => {
        //   console.log(`Current status is ${result.status}`);
 
    //     // Build a balance transfer extrinsic.
    // let dest = dev::bob().public_key().into();
    // let balance_transfer_tx = polkadot::tx().balances().transfer_allow_death(dest, 10_000);

    }
}

// #[derive(Debug, Clone)]
// pub struct RegisterParachainOptions {
//     para_id: u32,
//     wasm_path: AssetLocation,
//     state_path: AssetLocation,
//     api_url: AssetLocation,
//     onboard_as_para: bool,
//     seed: Option<String>,
//     finalization: bool
// }

// if (!parachain.addToGenesis && parachain.registerPara) {

// addToGenesis  = false
// registerPara  = true