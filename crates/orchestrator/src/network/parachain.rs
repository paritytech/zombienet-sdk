use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::anyhow;
use async_trait::async_trait;
use provider::types::TransferedFile;
use serde::{Deserialize, Serialize};
use subxt::{dynamic::Value, tx::TxStatus, OnlineClient, SubstrateConfig};
use subxt_signer::{sr25519::Keypair, SecretUri};
use support::{constants::THIS_IS_A_BUG, fs::FileSystem, net::wait_ws_ready};
use tracing::info;

use super::{chain_upgrade::ChainUpgrade, node::NetworkNode};
use crate::{
    network_spec::parachain::ParachainSpec,
    shared::types::{RegisterParachainOptions, RuntimeUpgradeOptions},
    tx_helper::client::get_client_from_url,
    utils::default_as_empty_vec,
    ScopedFilesystem,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Parachain {
    pub(crate) chain: Option<String>,
    pub(crate) para_id: u32,
    // unique_id is internally used to allow multiple parachains with the same id
    // See `ParachainConfig` for more details
    pub(crate) unique_id: String,
    pub(crate) chain_id: Option<String>,
    pub(crate) chain_spec_path: Option<PathBuf>,
    #[serde(default, deserialize_with = "default_as_empty_vec")]
    pub(crate) collators: Vec<NetworkNode>,
    #[serde(default)]
    pub(crate) files_to_inject: Vec<TransferedFile>,
    #[serde(default)]
    pub(crate) bootnodes_addresses: Vec<multiaddr::Multiaddr>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawParachain {
    #[serde(flatten)]
    pub(crate) inner: Parachain,
    pub(crate) collators: serde_json::Value,
}

#[async_trait]
impl ChainUpgrade for Parachain {
    async fn runtime_upgrade(&self, options: RuntimeUpgradeOptions) -> Result<(), anyhow::Error> {
        // check if the node is valid first
        let node = if let Some(node_name) = &options.node_name {
            if let Some(node) = self
                .collators()
                .into_iter()
                .find(|node| node.name() == node_name)
            {
                node
            } else {
                return Err(anyhow!("Node: {node_name} is not part of the set of nodes"));
            }
        } else {
            // take the first node
            if let Some(node) = self.collators().first() {
                node
            } else {
                return Err(anyhow!("chain doesn't have any node!"));
            }
        };

        self.perform_runtime_upgrade(node, options).await
    }
}

impl Parachain {
    pub(crate) fn new(para_id: u32, unique_id: impl Into<String>) -> Self {
        Self {
            chain: None,
            para_id,
            unique_id: unique_id.into(),
            chain_id: None,
            chain_spec_path: None,
            collators: Default::default(),
            files_to_inject: Default::default(),
            bootnodes_addresses: vec![],
        }
    }

    pub(crate) fn with_chain_spec(
        para_id: u32,
        unique_id: impl Into<String>,
        chain_id: impl Into<String>,
        chain_spec_path: impl AsRef<Path>,
    ) -> Self {
        Self {
            para_id,
            unique_id: unique_id.into(),
            chain: None,
            chain_id: Some(chain_id.into()),
            chain_spec_path: Some(chain_spec_path.as_ref().into()),
            collators: Default::default(),
            files_to_inject: Default::default(),
            bootnodes_addresses: vec![],
        }
    }

    pub(crate) async fn from_spec(
        para: &ParachainSpec,
        files_to_inject: &[TransferedFile],
        scoped_fs: &ScopedFilesystem<'_, impl FileSystem>,
    ) -> Result<Self, anyhow::Error> {
        let mut para_files_to_inject = files_to_inject.to_owned();

        // parachain id is used for the keystore
        let mut parachain = if let Some(chain_spec) = para.chain_spec.as_ref() {
            let id = chain_spec.read_chain_id(scoped_fs).await?;

            // add the spec to global files to inject
            let spec_name = chain_spec.chain_spec_name();
            let base = PathBuf::from_str(scoped_fs.base_dir)?;
            para_files_to_inject.push(TransferedFile::new(
                base.join(format!("{spec_name}.json")),
                PathBuf::from(format!("/cfg/{}.json", para.id)),
            ));

            let raw_path = chain_spec
                .raw_path()
                .ok_or(anyhow::anyhow!("chain-spec path should be set by now.",))?;
            let mut running_para =
                Parachain::with_chain_spec(para.id, &para.unique_id, id, raw_path);
            if let Some(chain_name) = chain_spec.chain_name() {
                running_para.chain = Some(chain_name.to_string());
            }

            running_para
        } else {
            Parachain::new(para.id, &para.unique_id)
        };

        parachain.bootnodes_addresses = para.bootnodes_addresses().into_iter().cloned().collect();
        parachain.files_to_inject = para_files_to_inject;

        Ok(parachain)
    }

    pub async fn register(
        options: RegisterParachainOptions,
        scoped_fs: &ScopedFilesystem<'_, impl FileSystem>,
    ) -> Result<(), anyhow::Error> {
        info!("Registering parachain: {:?}", options);
        // get the seed
        let sudo: Keypair;
        if let Some(possible_seed) = options.seed {
            sudo = Keypair::from_secret_key(possible_seed)
                .expect(&format!("seed should return a Keypair {THIS_IS_A_BUG}"));
        } else {
            let uri = SecretUri::from_str("//Alice")?;
            sudo = Keypair::from_uri(&uri)?;
        }

        let genesis_state = scoped_fs
            .read_to_string(options.state_path)
            .await
            .expect(&format!(
                "State Path should be ok by this point {THIS_IS_A_BUG}"
            ));
        let wasm_data = scoped_fs
            .read_to_string(options.wasm_path)
            .await
            .expect(&format!(
                "Wasm Path should be ok by this point {THIS_IS_A_BUG}"
            ));

        wait_ws_ready(options.node_ws_url.as_str())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Error waiting for ws to be ready, at {}",
                    options.node_ws_url.as_str()
                )
            })?;

        let api: OnlineClient<SubstrateConfig> = get_client_from_url(&options.node_ws_url).await?;

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
        let mut tx = api
            .tx()
            .sign_and_submit_then_watch_default(&sudo_call, &sudo)
            .await?;

        // Below we use the low level API to replicate the `wait_for_in_block` behaviour
        // which was removed in subxt 0.33.0. See https://github.com/paritytech/subxt/pull/1237.
        while let Some(status) = tx.next().await {
            match status? {
                TxStatus::InBestBlock(tx_in_block) | TxStatus::InFinalizedBlock(tx_in_block) => {
                    let _result = tx_in_block.wait_for_success().await?;
                    info!("In block: {:#?}", tx_in_block.block_hash());
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

    pub fn para_id(&self) -> u32 {
        self.para_id
    }

    pub fn unique_id(&self) -> &str {
        self.unique_id.as_str()
    }

    pub fn chain_id(&self) -> Option<&str> {
        self.chain_id.as_deref()
    }

    pub fn collators(&self) -> Vec<&NetworkNode> {
        self.collators.iter().collect()
    }

    pub fn bootnodes_addresses(&self) -> Vec<&multiaddr::Multiaddr> {
        self.bootnodes_addresses.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn create_with_is_works() {
        let para = Parachain::new(100, "100");
        // only para_id and unique_id should be set
        assert_eq!(para.para_id, 100);
        assert_eq!(para.unique_id, "100");
        assert_eq!(para.chain_id, None);
        assert_eq!(para.chain, None);
        assert_eq!(para.chain_spec_path, None);
    }

    #[test]
    fn create_with_chain_spec_works() {
        let para = Parachain::with_chain_spec(100, "100", "rococo-local", "/tmp/rococo-local.json");
        assert_eq!(para.para_id, 100);
        assert_eq!(para.unique_id, "100");
        assert_eq!(para.chain_id, Some("rococo-local".to_string()));
        assert_eq!(para.chain, None);
        assert_eq!(
            para.chain_spec_path,
            Some(PathBuf::from("/tmp/rococo-local.json"))
        );
    }

    #[tokio::test]
    async fn create_with_para_spec_works() {
        use configuration::ParachainConfigBuilder;

        use crate::network_spec::parachain::ParachainSpec;

        let bootnode_addresses = vec!["/ip4/10.41.122.55/tcp/45421"];

        let para_config = ParachainConfigBuilder::new(Default::default())
            .with_id(100)
            .cumulus_based(false)
            .with_default_command("adder-collator")
            .with_raw_bootnodes_addresses(bootnode_addresses.clone())
            .with_collator(|c| c.with_name("col"))
            .build()
            .unwrap();

        let para_spec =
            ParachainSpec::from_config(&para_config, "rococo-local".try_into().unwrap()).unwrap();
        let fs = support::fs::in_memory::InMemoryFileSystem::new(HashMap::default());
        let scoped_fs = ScopedFilesystem {
            fs: &fs,
            base_dir: "/tmp/some",
        };

        let files = vec![TransferedFile::new(
            PathBuf::from("/tmp/some"),
            PathBuf::from("/tmp/some"),
        )];
        let para = Parachain::from_spec(&para_spec, &files, &scoped_fs)
            .await
            .unwrap();
        println!("{para:#?}");
        assert_eq!(para.para_id, 100);
        assert_eq!(para.unique_id, "100");
        assert_eq!(para.chain_id, None);
        assert_eq!(para.chain, None);
        // one file should be added.
        assert_eq!(para.files_to_inject.len(), 1);
        assert_eq!(
            para.bootnodes_addresses()
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<_>>(),
            bootnode_addresses
        );
    }

    #[test]
    fn genesis_state_precedence_uses_path_over_generator() {
        use configuration::ParachainConfigBuilder;

        use crate::network_spec::parachain::ParachainSpec;

        let para_config = ParachainConfigBuilder::new(Default::default())
            .with_id(101)
            .with_genesis_state_path("./path/to/genesis/state")
            .with_genesis_state_generator("generator_state --flag")
            .with_collator(|c| c.with_name("col").with_command("cmd"))
            .build()
            .unwrap();

        let para_spec =
            ParachainSpec::from_config(&para_config, "relay".try_into().unwrap()).unwrap();

        // ParaArtifact implements Debug; ensure the build option is the Path variant
        let debug = format!("{:?}", para_spec.genesis_state);
        assert!(
            debug.contains("Path("),
            "expected genesis_state to be Path variant, got: {debug}"
        );
    }

    #[test]
    fn genesis_state_generator_with_args_preserved() {
        use configuration::ParachainConfigBuilder;

        use crate::network_spec::parachain::ParachainSpec;

        let para_config = ParachainConfigBuilder::new(Default::default())
            .with_id(102)
            .with_genesis_state_generator(
                "undying-collator export-genesis-state --pov-size=10000 --pvf-complexity=1",
            )
            .with_collator(|c| c.with_name("col").with_command("cmd"))
            .build()
            .unwrap();

        let para_spec =
            ParachainSpec::from_config(&para_config, "relay".try_into().unwrap()).unwrap();
        let debug = format!("{:?}", para_spec.genesis_state);

        // Ensure CommandWithCustomArgs is used and arguments are present in debug output
        assert!(
            debug.contains("CommandWithCustomArgs"),
            "expected CommandWithCustomArgs in debug, got: {debug}"
        );
        assert!(debug.contains("export-genesis-state"));
        assert!(debug.contains("--pov-size"));
    }
}
