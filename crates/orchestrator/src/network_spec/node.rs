use configuration::shared::{
    node::{EnvVar, NodeConfig},
    resources::Resources,
    types::{Arg, AssetLocation, Command, Image},
};
use multiaddr::Multiaddr;

use crate::{
    errors::OrchestratorError,
    generators,
    shared::types::{ChainDefaultContext, NodeAccounts, ParkedPort},
};


/// A node configuration, with fine-grained configuration options.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    /// Node name (should be unique or an index will be appended).
    pub(crate) name: String,

    /// Node key, used for compute the p2p identity.
    pub(crate) key: String,

    // libp2p local identity
    pub(crate) peer_id: String,

    /// Accounts to be injected in the keystore.
    pub(crate) accounts: NodeAccounts,

    /// Image to run (only podman/k8s). Override the default.
    pub(crate) image: Option<Image>,

    /// Command to run the node. Override the default.
    pub(crate) command: Command,

    /// Arguments to use for node. Appended to default.
    pub(crate) args: Vec<Arg>,

    /// Wether the node is a validator.
    pub(crate) is_validator: bool,

    /// Whether the node keys must be added to invulnerables.
    pub(crate) is_invulnerable: bool,

    /// Whether the node is a bootnode.
    pub(crate) is_bootnode: bool,

    /// Node initial balance present in genesis.
    pub(crate) initial_balance: u128,

    /// Environment variables to set (inside pod for podman/k8s, inside shell for native).
    pub(crate) env: Vec<EnvVar>,

    /// List of node's bootnodes addresses to use. Appended to default.
    pub(crate) bootnodes_addresses: Vec<Multiaddr>,

    /// Default resources. Override the default.
    pub(crate) resources: Option<Resources>,

    /// Websocket port to use.
    pub(crate) ws_port: ParkedPort,

    /// RPC port to use.
    pub(crate) rpc_port: ParkedPort,

    /// Prometheus port to use.
    pub(crate) prometheus_port: ParkedPort,

    /// P2P port to use.
    pub(crate) p2p_port: ParkedPort,

    /// libp2p cert hash to use with `webrtc` transport.
    pub(crate) p2p_cert_hash: Option<String>,

    /// Database snapshot. Override the default.
    pub(crate) db_snapshot: Option<AssetLocation>,
}

impl NodeSpec {
    pub fn from_config(
        node_config: &NodeConfig,
        chain_context: &ChainDefaultContext,
    ) -> Result<Self, OrchestratorError> {
        // Check first if the image is set at node level, then try with the default
        let image = if let Some(img) = node_config.image() {
            Some(img.clone())
        } else {
            chain_context.default_image.cloned()
        };

        // Check first if the command is set at node level, then try with the default
        let command = if let Some(cmd) = node_config.command() {
            cmd.clone()
        } else if let Some(cmd) = chain_context.default_command {
            cmd.clone()
        } else {
            return Err(OrchestratorError::InvalidNodeConfig(
                node_config.name().into(),
                "command".to_string(),
            ));
        };

        // If `args` is set at `node` level use them
        // otherwise use the default_args (can be empty).
        let args: Vec<Arg> = if node_config.args().is_empty() {
            chain_context
                .default_args
                .iter()
                .map(|x| x.to_owned().clone())
                .collect()
        } else {
            node_config.args().into_iter().cloned().collect()
        };


        let (key, peer_id) = generators::identity::generate_for_node(node_config.name())?;

        let mut name = node_config.name().to_string();
        let seed = format!("//{}{name}", name.remove(0).to_uppercase());
        let accounts = generators::key::generate_for_node(&seed)?;
        let accounts = NodeAccounts { seed, accounts };

        //
        Ok(Self {
            name: node_config.name().to_string(),
            key,
            peer_id,
            image,
            command,
            args,
            is_validator: node_config.is_validator(),
            is_invulnerable: node_config.is_invulnerable(),
            is_bootnode: node_config.is_bootnode(),
            initial_balance: node_config.initial_balance(),
            env: node_config.env().into_iter().cloned().collect(),
            bootnodes_addresses: node_config
                .bootnodes_addresses()
                .into_iter()
                .cloned()
                .collect(),
            resources: node_config.resources().cloned(),
            p2p_cert_hash: node_config.p2p_cert_hash().map(str::to_string),
            db_snapshot: node_config.db_snapshot().cloned(),
            accounts,
            ws_port: generators::port::generate(node_config.ws_port())?,
            rpc_port: generators::port::generate(node_config.rpc_port())?,
            prometheus_port: generators::port::generate(node_config.prometheus_port())?,
            p2p_port: generators::port::generate(node_config.p2p_port())?,
        })
    }
}