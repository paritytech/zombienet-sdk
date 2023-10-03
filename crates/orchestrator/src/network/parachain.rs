use std::{
    fs,
    path::{Path, PathBuf},
};

use subxt::{dynamic::Value, OnlineClient, SubstrateConfig};
use subxt_signer::sr25519::Keypair;

use super::node::NetworkNode;
use crate::shared::types::{ParachainGenesisArgs, RegisterParachainOptions};

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

    pub async fn register(
        options: RegisterParachainOptions,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Registering parachain: {:?}", options);
        // get the seed
        let seed: [u8; 32];
        if let Some(possible_seed) = options.seed {
            seed = possible_seed;
        } else {
            seed = b"//Alice".to_vec().try_into().unwrap();
        }
        // get the sudo account for registering the parachain
        let sudo = Keypair::from_seed(seed).unwrap();

        let genesis_state = fs::read_to_string(options.state_path).unwrap();
        let wasm_data = fs::read_to_string(options.wasm_path).unwrap();

        let parachain_genesis_value = Value::from_bytes(ParachainGenesisArgs {
            genesis_head: genesis_state,
            validation_code: wasm_data,
            parachain: options.onboard_as_para,
        });

        let api = OnlineClient::<SubstrateConfig>::from_url(options.node_ws_url).await?;

        // based on subXT docs: The public key bytes are equivalent to a Substrate `AccountId32`;
        let account_id_value = Value::from_bytes(sudo.public_key());

        // get the nonce for the sudo account
        let account_nonce_call = subxt::dynamic::runtime_api_call(
            "AccountNonceApi",
            "account_nonce",
            vec![account_id_value.clone()],
        );

        let nonce = api
            .runtime_api()
            .at_latest()
            .await?
            .call(account_nonce_call)
            .await?;

        println!("Account nonce: {:#?}", nonce.to_value());

        //
        let schedule_para = subxt::dynamic::runtime_api_call(
            "ParasSudoWrapperCall",
            "sudo_schedule_para_initialize",
            vec![account_id_value, parachain_genesis_value],
        )
        .into();

        // TODO: uncomment below and fix the sign and submit (and follow afterwards until
        // finalized block) to register the parachain
        let result = api
            .tx()
            .sign_and_submit_then_watch_default(&schedule_para, &sudo)
            .await?;

        Ok(())
    }
}
