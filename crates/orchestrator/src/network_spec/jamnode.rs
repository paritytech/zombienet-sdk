use configuration::{
    shared::{
        node::{EnvVar, JamNodeConfig},
        resources::Resources,
        types::{Arg, Command, Image},
    },
    types::JamNodeMode,
};
use jam_std_common::{ed25519, PeerId};
use jam_types::hex;
use serde::{Deserialize, Serialize};

use crate::{
    errors::OrchestratorError,
    generators,
    shared::types::{ChainDefaultContext, NodeAccounts, ParkedPort},
};

/// A node configuration, with fine-grained configuration options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JamNodeSpec {
    // Node name (should be unique or an index will be appended).
    pub(crate) name: String,

    /// node mode.
    pub(crate) mode: JamNodeMode,

    // libp2p local identity
    pub(crate) peer_id: String,

    /// Accounts to be injected in the keystore.
    pub(crate) accounts: NodeAccounts,

    /// Image to run (only podman/k8s). Override the default.
    pub(crate) image: Option<Image>,

    /// Command to run the node. Override the default.
    pub(crate) command: Command,

    /// Optional subcommand for the node.
    pub(crate) subcommand: Option<Command>,

    /// Arguments to use for node. Appended to default.
    pub(crate) args: Vec<Arg>,

    /// Environment variables to set (inside pod for podman/k8s, inside shell for native).
    pub(crate) env: Vec<EnvVar>,

    /// Default resources. Override the default.
    pub(crate) resources: Option<Resources>,

    /// Port to use.
    pub(crate) port: ParkedPort,

    /// RPC port to use.
    pub(crate) rpc_port: ParkedPort,

    /// Telemetry endpoint
    pub(crate) telemetry_endpoint: Option<String>,
}

impl JamNodeSpec {
    pub fn from_config(
        node_config: &JamNodeConfig,
        chain_context: &ChainDefaultContext,
    ) -> Result<Self, OrchestratorError> {
        // Check first if the image is set at node level, then try with the default
        let image = node_config.image().or(chain_context.default_image).cloned();

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

        let subcommand = node_config.subcommand().cloned();

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

        let mut name = node_config.name().to_string();
        let seed = format!("{}{name}", name.remove(0).to_uppercase());
        let accounts = generators::generate_jam_node_keys(&seed)?;
        let accounts = NodeAccounts {
            seed: seed.clone(),
            accounts,
        };
        println!("{:?}", accounts);
        let ed25519 = accounts
            .accounts
            .get("ed25519")
            .expect("ed25519 key should be present.");
        let ed25519_b: [u8; 32] = hex::from_hex(&ed25519.public_key)
            .expect("ed25519 should be valid")
            .try_into()
            .expect("ed25519 should be valid and convert to [u8;32]");
        let ed25519_pub = ed25519::Public::from(ed25519_b);
        let peer_id = PeerId(ed25519_pub);

        Ok(Self {
            name: node_config.name().to_string(),
            mode: node_config.mode().clone(),
            peer_id: peer_id.to_string(),
            image,
            command,
            subcommand,
            args,
            env: node_config.env().into_iter().cloned().collect(),
            resources: node_config.resources().cloned(),
            accounts,
            port: generators::generate_node_port(None)?,
            rpc_port: generators::generate_node_port(node_config.rpc_port())?,
            telemetry_endpoint: node_config.telemetry_endpoint().map(str::to_string),
        })
    }

    pub fn command(&self) -> &str {
        self.command.as_str()
    }

    pub fn peer_id(&self) -> &str {
        self.peer_id.as_str()
    }
}
