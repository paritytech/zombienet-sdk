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
    shared::types::ChainDefaultContext,
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

#[derive(Default, Debug, Clone)]
pub struct AddNodeOpts {
    pub image: Option<Image>,
    pub command: Option<Command>,
    pub args: Vec<Arg>,
    pub env: Vec<EnvVar>,
    pub is_validator: bool,
    pub rpc_port: Option<Port>,
    pub prometheus_port: Option<Port>,
    pub p2p_port: Option<Port>,
}

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

    // Pub API

    // Teardown the network
    // destroy()

    pub async fn add_node(
        &mut self,
        name: impl Into<String>,
        options: AddNodeOpts,
        para_id: Option<u32>,
    ) -> Result<(), anyhow::Error> {
        // build context
        // let (maybe_para_chain_id, chain_context, para_spec, role) =
        let (chain_context, role, maybe_para_chain_id, para_spec, maybe_para_chain_spec_path) =
            if let Some(para_id) = para_id {
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

                // (parachain.chain_id.clone(), chain_context, Some(spec), role)
                (
                    chain_context,
                    role,
                    parachain.chain_id.clone(),
                    Some(spec),
                    parachain.chain_spec_path.clone(),
                )
            } else {
                let spec = &self.initial_spec.relaychain;
                let chain_context = ChainDefaultContext {
                    default_command: spec.default_command.as_ref(),
                    default_image: spec.default_image.as_ref(),
                    default_resources: spec.default_resources.as_ref(),
                    default_db_snapshot: spec.default_db_snapshot.as_ref(),
                    default_args: spec.default_args.iter().collect(),
                };
                (chain_context, ZombieRole::Node, None, None, None)
            };

        let node_spec =
            network_spec::node::NodeSpec::from_ad_hoc(name.into(), options, &chain_context)?;
        let base_dir = self.ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        // TODO: we want to still supporting spawn a dedicated bootnode??
        let ctx = SpawnNodeCtx {
            chain_id: &self.relay.chain_id,
            parachain_id: maybe_para_chain_id.as_deref(),
            chain: &self.relay.chain,
            role,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            parachain: para_spec,
            bootnodes_addr: &vec![],
            wait_ready: true,
        };

        let mut global_files_to_inject = vec![TransferedFile::new(
            PathBuf::from(format!(
                "{}/{}.json",
                self.ns.base_dir().to_string_lossy(),
                self.relay.chain
            )),
            PathBuf::from(format!("/cfg/{}.json", self.relay.chain)),
        )];

        if let Some(para_spec_path) = maybe_para_chain_spec_path {
            global_files_to_inject.push(TransferedFile::new(
                PathBuf::from(format!(
                    "{}/{}",
                    self.ns.base_dir().to_string_lossy(),
                    para_spec_path.to_string_lossy()
                )),
                PathBuf::from(format!(
                    "/cfg/{}.json",
                    para_id.ok_or(anyhow::anyhow!(
                        "para_id should be valid here, this is a bug!"
                    ))?
                )),
            ));
        }

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;
        self.add_running_node(node, None);

        Ok(())
    }

    // This should include at least of collator?
    // add_parachain()

    // deregister and stop the collator?
    // remove_parachain()

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

    pub(crate) fn name(&self) -> &str {
        self.ns.name()
    }

    pub(crate) fn relaychain(&self) -> &Relaychain {
        &self.relay
    }

    pub(crate) fn parachain(&self, para_id: u32) -> Option<&Parachain> {
        self.parachains.get(&para_id)
    }

    pub(crate) fn parachains(&self) -> Vec<&Parachain> {
        self.parachains.values().collect()
    }
}
