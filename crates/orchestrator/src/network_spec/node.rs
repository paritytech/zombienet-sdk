use configuration::shared::{
    node::{EnvVar, NodeConfig},
    resources::Resources,
    types::{Arg, AssetLocation, Command, Image, Port},
};
use hex;
use multiaddr::Multiaddr;
use sha2::digest::Digest;

use crate::{
    errors::OrchestratorError,
    generators,
    shared::types::{NodeAccounts, ParkedPort, ChainDefaultContext},
};

/// A node configuration, with fine-grained configuration options.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    /// Node name (should be unique or an index will be appended).
    name: String,

    /// Node key, used for compute the p2p identity.
    key: String,

    /// Accounts to be injected in the keystore.
    accounts: NodeAccounts,

    /// Image to run (only podman/k8s). Override the default.
    image: Option<Image>,

    /// Command to run the node. Override the default.
    command: Command,

    /// Arguments to use for node. Appended to default.
    args: Vec<Arg>,

    /// Wether the node is a validator.
    is_validator: bool,

    /// Whether the node keys must be added to invulnerables.
    is_invulnerable: bool,

    /// Whether the node is a bootnode.
    is_bootnode: bool,

    /// Node initial balance present in genesis.
    initial_balance: u128,

    /// Environment variables to set (inside pod for podman/k8s, inside shell for native).
    env: Vec<EnvVar>,

    /// List of node's bootnodes addresses to use. Appended to default.
    bootnodes_addresses: Vec<Multiaddr>,

    /// Default resources. Override the default.
    resources: Option<Resources>,

    /// Websocket port to use.
    ws_port: ParkedPort,

    /// RPC port to use.
    rpc_port: ParkedPort,

    /// Prometheus port to use.
    prometheus_port: ParkedPort,

    /// P2P port to use.
    p2p_port: ParkedPort,

    /// libp2p cert hash to use with `webrtc` transport.
    p2p_cert_hash: Option<String>,

    /// Database snapshot. Override the default.
    db_snapshot: Option<AssetLocation>,
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
            if let Some(img) = chain_context.default_image {
                Some(img.clone())
            } else {
                None
            }
        };

        // Check first if the command is set at node level, then try with the default
        let command = if let Some(cmd) = node_config.command() {
            cmd.clone()
        } else {
            if let Some(cmd) = chain_context.default_command {
                cmd.clone()
            } else {
                return Err(OrchestratorError::InvalidNodeConfig(
                    node_config.name().into(),
                    "command".to_string(),
                ));
            }
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

        let key = hex::encode(sha2::Sha256::digest(node_config.name()));
        let seed = format!("//{}", node_config.name());
        let accounts = generators::key::generate_for_node(&seed)?;
        let accounts = NodeAccounts { seed, accounts };

        //
        Ok(Self {
            name: node_config.name().to_string(),
            key,
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
