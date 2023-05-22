use crate::shared::types::{
    Arg, Command, ContainerImage, DbSnapshot, MultiAddress, Port, Resources,
};

#[derive(Debug, Clone)]
struct EnvVar {
    name:  String,
    value: String,
}

impl From<(String, String)> for EnvVar {
    fn from((name, value): (String, String)) -> Self {
        Self { name, value }
    }
}

/// A node configuration, with fine-grained configuration options.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Node name (should be unique or an index will be appended).
    name: String,

    /// Image to run (only podman/k8s). Override the default.
    image: Option<ContainerImage>,

    /// Command to run the node. Override the default.
    command: Option<Command>,

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
    bootnodes_addresses: Vec<MultiAddress>,

    /// Default resources. Override the default.
    resources: Option<Resources>,

    /// Websocket port to use. Default to 9944 + n where n is the node index in the network (starting from 0).
    ws_port: Option<Port>,

    /// RPC port to use. Default to 9933 + n where n is the node index in the network (starting from 0).
    // [TODO]: start at a different default to avoid overlap between ws_port and rpc_port when node count >= 12 ?
    rpc_port: Option<Port>,

    /// Prometheus port to use. Default to 9615 + n where n is the node index in the network (starting from 0).
    prometheus_port: Option<Port>,

    /// P2P port to use. Default to 30333 + n where n is the node index in the network (starting from 0)
    p2p_port: Option<Port>,

    /// libp2p cert hash to use with `webrtc` transport.
    p2p_cert_hash: Option<String>,

    /// Database snapshot. Override the default.
    db_snapshot: Option<DbSnapshot>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        // [TODO]: define the default value for a node
        todo!()
    }
}
