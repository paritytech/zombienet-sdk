use std::path::{Path, PathBuf};

use subxt::{dynamic::Value, OnlineClient, SubstrateConfig};
use subxt_signer::sr25519::Keypair;
use support::fs::FileSystem;

// use crate::generators::key::generate_pair;
// use sp_core::{sr25519, Pair};
use super::node::NetworkNode;
use crate::{
    shared::types::{ParachainGenesisArgs, RegisterParachainOptions},
    ScopedFilesystem,
};

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

    pub async fn register<'a, T>(
        options: RegisterParachainOptions,
        scoped_fs: &ScopedFilesystem<'a, impl FileSystem>,
    ) -> Result<(), anyhow::Error> {
        println!("Registering parachain: {:?}", options);
        // get the seed
        let seed: [u8; 32];

        if let Some(possible_seed) = options.seed {
            seed = possible_seed;
        } else {
            seed = b"//Alice"
                .to_vec()
                .try_into()
                .expect("Alice seed should be ok at this point.")
        }
        // TODO: the Keypair should come from the generators instead of the subxt-signer
        let sudo = Keypair::from_seed(seed).expect("seed should return a Keypair.");

        let genesis_state = scoped_fs
            .read_to_string(options.state_path)
            .await
            .expect("State Path should be ok by this point.");
        let wasm_data = scoped_fs
            .read_to_string(options.wasm_path)
            .await
            .expect("Wasm Path should be ok by this point.");

        let parachain_genesis_value: ParachainGenesisArgs = ParachainGenesisArgs {
            genesis_head: genesis_state,
            validation_code: wasm_data,
            parachain: options.onboard_as_para,
            // TODO: this is probably not correct - just a workaround for now
            // it is intenionally empty as a workaround for AsRef and Deref
            encoded: vec![],
        };

        let api = OnlineClient::<SubstrateConfig>::from_url(options.node_ws_url).await?;

        // // based on subXT docs: The public key bytes are equivalent to a Substrate `AccountId32`;
        let account_id = sudo.public_key();

        let schedule_para = subxt::dynamic::tx(
            "ParasSudoWrapperCall",
            "sudo_schedule_para_initialize",
            vec![
                Value::from_bytes(account_id),
                Value::from_bytes(parachain_genesis_value),
            ],
        );

        // TODO: uncomment below and fix the sign and submit (and follow afterwards until
        // finalized block) to register the parachain
        let result = api
            .tx()
            .sign_and_submit_then_watch_default(&schedule_para, &sudo)
            .await?;

        println!("{:#?}", result);
        Ok(())
    }
}
