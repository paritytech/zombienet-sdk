pub mod node;
pub mod parachain;
pub mod relaychain;

use std::{collections::HashMap, path::PathBuf};

use configuration::{
    shared::node::EnvVar,
    types::{Arg, Command, Image, Port},
};
use provider::{types::TransferedFile, DynNamespace};
use support::fs::FileSystem;

use self::{node::NetworkNode, parachain::Parachain, relaychain::Relaychain};
use crate::{
    network_spec::{self, NetworkSpec},
    shared::{macros, types::ChainDefaultContext},
    spawner::{self, SpawnNodeCtx},
    ScopedFilesystem, ZombieRole,
};

pub struct Network<T: FileSystem> {
    ns: DynNamespace,
    filesystem: T,
    relay: Relaychain,
    initial_spec: NetworkSpec,
    parachains: HashMap<u32, Parachain>,
    nodes_by_name: HashMap<String, NetworkNode>,
}

impl<T: FileSystem> std::fmt::Debug for Network<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network")
            .field("ns", &"ns_skipped")
            .field("relay", &self.relay)
            .field("initial_spec", &self.initial_spec)
            .field("parachains", &self.parachains)
            .field("nodes_by_name", &self.nodes_by_name)
            .finish()
    }
}

macros::create_add_options!(AddNodeOptions {
    chain_spec: Option<PathBuf>
});

macros::create_add_options!(AddCollatorOptions {
    chain_spec: Option<PathBuf>,
    chain_spec_relay: Option<PathBuf>
});

impl<T: FileSystem> Network<T> {
    pub(crate) fn new_with_relay(
        relay: Relaychain,
        ns: DynNamespace,
        fs: T,
        initial_spec: NetworkSpec,
    ) -> Self {
        Self {
            ns,
            filesystem: fs,
            relay,
            initial_spec,
            parachains: Default::default(),
            nodes_by_name: Default::default(),
        }
    }

    // Pubic API

    pub fn relaychain(&self) -> &Relaychain {
        &self.relay
    }

    // Teardown the network
    // destroy()

    /// Add a node to the relaychain
    ///
    /// NOTE: name must be unique in the whole network. The new node is added to the
    /// running network instance.
    ///
    /// # Example:
    /// ```rust
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem, process::os::OsProcessManager};
    /// # use zombienet_orchestrator::{errors, AddNodeOptions, Orchestrator};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), errors::OrchestratorError> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {}, OsProcessManager {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    ///     let mut network = orchestrator.spawn(config).await?;
    ///
    ///     // Create the options to add the new node
    ///     let opts = AddNodeOptions {
    ///         rpc_port: Some(9444),
    ///         is_validator: true,
    ///         ..Default::default()
    ///     };
    ///
    ///     network.add_node("new-node", opts).await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn add_node(
        &mut self,
        name: impl Into<String>,
        options: AddNodeOptions,
    ) -> Result<(), anyhow::Error> {
        let name = name.into();
        let relaychain = self.relaychain();

        if self.nodes_by_name.contains_key(&name) {
            return Err(anyhow::anyhow!("Name: {} is already used.", name));
        }

        let chain_spec_path = if let Some(chain_spec_custom_path) = &options.chain_spec {
            chain_spec_custom_path.clone()
        } else {
            PathBuf::from(format!(
                "{}/{}.json",
                self.ns.base_dir().to_string_lossy(),
                relaychain.chain
            ))
        };

        let chain_context = ChainDefaultContext {
            default_command: self.initial_spec.relaychain.default_command.as_ref(),
            default_image: self.initial_spec.relaychain.default_image.as_ref(),
            default_resources: self.initial_spec.relaychain.default_resources.as_ref(),
            default_db_snapshot: self.initial_spec.relaychain.default_db_snapshot.as_ref(),
            default_args: self.initial_spec.relaychain.default_args.iter().collect(),
        };

        let node_spec =
            network_spec::node::NodeSpec::from_ad_hoc(&name, options.into(), &chain_context)?;
        let base_dir = self.ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        let ctx = SpawnNodeCtx {
            chain_id: &relaychain.chain_id,
            parachain_id: None,
            chain: &relaychain.chain,
            role: ZombieRole::Node,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            parachain: None,
            bootnodes_addr: &vec![],
            wait_ready: true,
        };

        let global_files_to_inject = vec![TransferedFile {
            local_path: chain_spec_path,
            remote_path: PathBuf::from(format!("/cfg/{}.json", relaychain.chain)),
        }];

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;

        // TODO: register the new node as validator in the relaychain
        // STEPS:
        //  - check balance of `stash` derivation for validator account
        //  - call rotate_keys on the new validator
        //  - call setKeys on the new validator
        // if node_spec.is_validator {
        //     let running_node = self.relay.nodes.first().unwrap();
        //     // tx_helper::validator_actions::register(vec![&node], &running_node.ws_uri, None).await?;
        // }

        // Add node to the global hash
        self.add_running_node(node.clone(), None);
        // add node to relay
        self.relay.nodes.push(node);

        Ok(())
    }

    /// Add a new collator to a parachain
    ///
    /// NOTE: name must be unique in the whole network.
    ///
    /// # Example:
    /// ```rust
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem, process::os::OsProcessManager};
    /// # use zombienet_orchestrator::{errors, AddCollatorOptions, Orchestrator};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), anyhow::Error> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {}, OsProcessManager {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    ///     let mut network = orchestrator.spawn(config).await?;
    ///
    ///     let col_opts = AddCollatorOptions {
    ///         command: Some("polkadot-parachain".try_into()?),
    ///         ..Default::default()
    ///     };
    ///
    ///     network.add_collator("new-col-1", col_opts, 100).await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn add_collator(
        &mut self,
        name: impl Into<String>,
        options: AddCollatorOptions,
        para_id: u32,
    ) -> Result<(), anyhow::Error> {
        let spec = self
            .initial_spec
            .parachains
            .iter()
            .find(|para| para.id == para_id)
            .ok_or(anyhow::anyhow!(format!("parachain: {para_id} not found!")))?;
        let role = if spec.is_cumulus_based {
            ZombieRole::CumulusCollator
        } else {
            ZombieRole::Collator
        };
        let chain_context = ChainDefaultContext {
            default_command: spec.default_command.as_ref(),
            default_image: spec.default_image.as_ref(),
            default_resources: spec.default_resources.as_ref(),
            default_db_snapshot: spec.default_db_snapshot.as_ref(),
            default_args: spec.default_args.iter().collect(),
        };
        let parachain = self
            .parachains
            .get(&para_id)
            .ok_or(anyhow::anyhow!(format!("parachain: {para_id} not found!")))?;

        let base_dir = self.ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        // TODO: we want to still supporting spawn a dedicated bootnode??
        let ctx = SpawnNodeCtx {
            chain_id: &self.relay.chain_id,
            parachain_id: parachain.chain_id.as_deref(),
            chain: &self.relay.chain,
            role,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            parachain: Some(spec),
            bootnodes_addr: &vec![],
            wait_ready: true,
        };

        let relaychain_spec_path = if let Some(chain_spec_custom_path) = &options.chain_spec_relay {
            chain_spec_custom_path.clone()
        } else {
            PathBuf::from(format!(
                "{}/{}.json",
                self.ns.base_dir().to_string_lossy(),
                self.relay.chain
            ))
        };

        let mut global_files_to_inject = vec![TransferedFile {
            local_path: relaychain_spec_path,
            remote_path: PathBuf::from(format!("/cfg/{}.json", self.relay.chain)),
        }];

        let para_chain_spec_local_path = if let Some(para_chain_spec_custom) = &options.chain_spec {
            Some(para_chain_spec_custom.clone())
        } else {
            if let Some(para_spec_path) = &parachain.chain_spec_path {
                Some(PathBuf::from(format!(
                    "{}/{}",
                    self.ns.base_dir().to_string_lossy(),
                    para_spec_path.to_string_lossy()
                )))
            } else {
                None
            }
        };

        if let Some(para_spec_path) = para_chain_spec_local_path {
            global_files_to_inject.push(TransferedFile {
                local_path: para_spec_path,
                remote_path: PathBuf::from(format!("/cfg/{}.json", para_id)),
            });
        }

        let node_spec =
            network_spec::node::NodeSpec::from_ad_hoc(name.into(), options.into(), &chain_context)?;

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;
        let para = self.parachains.get_mut(&para_id).unwrap();
        para.collators.push(node.clone());
        self.add_running_node(node, None);

        Ok(())
    }


    // This should include at least of collator?
    // add_parachain()

    // deregister and stop the collator?
    // remove_parachain()

    pub fn get_node(&self, name: impl Into<String>) -> Result<&NetworkNode, anyhow::Error> {
        let name = &name.into();
        if let Some(node) = self.nodes_by_name.get(name) {
            return Ok(node);
        }

        let list = self
            .nodes_by_name
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        Err(anyhow::anyhow!(
            "can't find node with name: {name:?}, should be one of {list}"
        ))
    }

    pub fn nodes(&self) -> Vec<&NetworkNode> {
        self.nodes_by_name.values().collect::<Vec<&NetworkNode>>()
    }

    // Internal API
    pub(crate) fn add_running_node(&mut self, node: NetworkNode, para_id: Option<u32>) {
        if let Some(para_id) = para_id {
            if let Some(para) = self.parachains.get_mut(&para_id) {
                para.collators.push(node.clone());
            } else {
                unreachable!()
            }
        } else {
            self.relay.nodes.push(node.clone());
        }
        // TODO: we should hold a ref to the node in the vec in the future.
        let node_name = node.name.clone();
        self.nodes_by_name.insert(node_name, node);
    }

    pub(crate) fn add_para(&mut self, para: Parachain) {
        self.parachains.insert(para.para_id, para);
    }

    pub(crate) fn name(&self) -> &str {
        self.ns.name()
    }

    pub(crate) fn parachain(&self, para_id: u32) -> Option<&Parachain> {
        self.parachains.get(&para_id)
    }

    pub(crate) fn parachains(&self) -> Vec<&Parachain> {
        self.parachains.values().collect()
    }
}
