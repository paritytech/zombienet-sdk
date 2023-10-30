pub mod node;
pub mod parachain;
pub mod relaychain;

use std::{collections::HashMap, path::PathBuf};

use configuration::{
    shared::node::EnvVar,
    types::{Arg, Command, Image, Port, ParaId},
};
use provider::{types::TransferedFile, DynNamespace};
use support::fs::FileSystem;

use self::{node::NetworkNode, parachain::Parachain, relaychain::Relaychain};
use crate::{
    network_spec::{self, NetworkSpec},
    shared::types::ChainDefaultContext,
    spawner::{self, SpawnNodeCtx},
    ScopedFilesystem, ZombieRole, tx_helper,
    shared::{macros, types::AddParaOpts}
};


pub struct Network<FS: FileSystem> {
    ns: DynNamespace,
    filesystem: FS,
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

impl<FS: FileSystem> Network<FS> {
    pub(crate) fn new_with_relay(
        relay: Relaychain<FS>,
        ns: DynNamespace,
        fs: FS,
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

    // Public API
    pub fn relaychain(&self) -> &Relaychain<FS> {
        &self.relay
    }

    /// Add a node to the relaychain
    /// NOTE: name must be unique in the whole network.
    pub async fn add_node(&mut self, name: impl Into<String>, options: AddNodeOptions) -> Result<(), anyhow::Error> {
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

        // build context
        let chain_context = ChainDefaultContext {
            default_command: relaychain.initial_spec.default_command.as_ref(),
            default_image: relaychain.initial_spec.default_image.as_ref(),
            default_resources: relaychain.initial_spec.default_resources.as_ref(),
            default_db_snapshot: relaychain.initial_spec.default_db_snapshot.as_ref(),
            default_args: relaychain.initial_spec.default_args.iter().collect(),
        };

        let role = ZombieRole::Node;


        let node_spec =
            network_spec::node::NodeSpec::from_ad_hoc(&name, options.into(), &chain_context)?;
        let base_dir = self.ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        let ctx = SpawnNodeCtx {
            chain_id: &relaychain.chain_id,
            parachain_id: None,
            chain: &relaychain.chain,
            role,
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

        // TODO
        // if node_spec.is_validator {
        //     let running_node = self.relay.nodes.first().unwrap();
        //     // register them
        //     // check if the stash account have funds
        //     // IF not transfer balance
        //     // call rotate_keys
        //     // call setKeys
        //     // tx_helper::validator_actions::register(vec![&node], &running_node.ws_uri, None).await.expect("errr!!!!!!");
        // }

        // Add node to the global hash
        self.add_running_node(node.clone(), None);
        // add node to relay
        self.relay.nodes.push(node);

        Ok(())
    }

    pub async fn add_collator(&mut self, name: impl Into<String>, options: AddCollatorOptions, para_id: u32) -> Result<(), anyhow::Error> {
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
                local_path:para_spec_path,
                remote_path: PathBuf::from(format!(
                    "/cfg/{}.json",
                    para_id
                )),
            });
        }

        let node_spec =
            network_spec::node::NodeSpec::from_ad_hoc(name.into(), options.into(), &chain_context)?;

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;
        let para  = self.parachains.get_mut(&para_id).unwrap();
        para.collators.push(node.clone());
        self.add_running_node(node, None);

        Ok(())
    }

    pub async fn add_parachain(&mut self, para_id: ParaId, para_config: AddParaOpts, custom_relaychain_spec: Option<PathBuf> ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    // deregister and stop the collators
    // remove_parachain()

    // Teardown the network
    // destroy()


    pub fn get_node(&self, node_name: impl Into<String>) -> Result<&NetworkNode, anyhow::Error> {
        let node_name = node_name.into();
        if let Some(node) = self.nodes_by_name.get(&node_name) {
            Ok(node)
        } else {
            Err(anyhow::anyhow!("can't find the node!"))
        }
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

    pub(crate) fn id(&self) -> &str {
        self.ns.id()
    }

    pub(crate) fn parachain(&self, para_id: u32) -> Option<&Parachain> {
        self.parachains.get(&para_id)
    }

    pub(crate) fn parachains(&self) -> Vec<&Parachain> {
        self.parachains.values().collect()
    }
}
