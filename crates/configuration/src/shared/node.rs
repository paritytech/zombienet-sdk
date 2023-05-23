use crate::shared::types::{Arg, Command, ContainerImage, MultiAddress, Port, Resources};

use super::types::AssetLocation;

#[derive(Debug, Clone)]
pub struct EnvVar {
    name: String,
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
    db_snapshot: Option<AssetLocation>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        // [TODO]: define the default value for a node
        todo!()
    }
}

impl NodeConfig {
    pub fn with_name(self, name: String) -> Self {
        Self { name, ..self }
    }

    pub fn with_image(self, image: ContainerImage) -> Self {
        Self {
            image: Some(image),
            ..self
        }
    }

    pub fn with_command(self, command: Command) -> Self {
        Self {
            command: Some(command),
            ..self
        }
    }

    pub fn with_args(self, args: Vec<Arg>) -> Self {
        Self { args, ..self }
    }

    pub fn as_validator(self) -> Self {
        Self {
            is_validator: true,
            ..self
        }
    }

    pub fn as_invulnerable(self) -> Self {
        Self {
            is_invulnerable: true,
            ..self
        }
    }

    pub fn as_bootnode(self) -> Self {
        Self {
            is_bootnode: true,
            ..self
        }
    }

    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self {
            initial_balance,
            ..self
        }
    }

    pub fn with_env(self, env: Vec<EnvVar>) -> Self {
        Self { env, ..self }
    }

    pub fn with_bootnodes_addresses(self, bootnodes_addresses: Vec<MultiAddress>) -> Self {
        Self {
            bootnodes_addresses,
            ..self
        }
    }

    pub fn with_resources(self, f: fn(Resources) -> Resources) -> Self {
        Self {
            resources: Some(f(Resources::default())),
            ..self
        }
    }

    pub fn with_ws_port(self, ws_port: Port) -> Self {
        Self {
            ws_port: Some(ws_port),
            ..self
        }
    }

    pub fn with_rpc_port(self, rpc_port: Port) -> Self {
        Self {
            rpc_port: Some(rpc_port),
            ..self
        }
    }

    pub fn with_prometheus_port(self, prometheus_port: Port) -> Self {
        Self {
            prometheus_port: Some(prometheus_port),
            ..self
        }
    }

    pub fn with_p2p_port(self, p2p_port: Port) -> Self {
        Self {
            p2p_port: Some(p2p_port),
            ..self
        }
    }

    pub fn with_p2p_cert_hash(self, p2p_cert_hash: String) -> Self {
        Self {
            p2p_cert_hash: Some(p2p_cert_hash),
            ..self
        }
    }

    pub fn with_db_snapshot(self, location: AssetLocation) -> Self {
        Self {
            db_snapshot: Some(location),
            ..self
        }
    }
}
