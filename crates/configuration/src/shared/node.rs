use std::marker::PhantomData;

use multiaddr::Multiaddr;

use super::{macros::states, resources::ResourcesBuilder, types::AssetLocation};
use crate::shared::{
    resources::Resources,
    types::{Arg, Port},
};

#[derive(Debug, Clone, PartialEq)]
pub struct EnvVar {
    name: String,
    value: String,
}

impl From<(&str, &str)> for EnvVar {
    fn from((name, value): (&str, &str)) -> Self {
        Self {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }
}

/// A node configuration, with fine-grained configuration options.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeConfig {
    /// Node name (should be unique or an index will be appended).
    name: String,

    /// Image to run (only podman/k8s). Override the default.
    image: Option<String>,

    /// Command to run the node. Override the default.
    command: Option<String>,

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

impl NodeConfig {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn image(&self) -> Option<&str> {
        self.image.as_deref()
    }

    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    pub fn args(&self) -> Vec<&Arg> {
        self.args.iter().collect()
    }

    pub fn is_validator(&self) -> bool {
        self.is_validator
    }

    pub fn is_invulnerable(&self) -> bool {
        self.is_invulnerable
    }

    pub fn is_bootnode(&self) -> bool {
        self.is_bootnode
    }

    pub fn initial_balance(&self) -> u128 {
        self.initial_balance
    }

    pub fn env(&self) -> Vec<&EnvVar> {
        self.env.iter().collect()
    }

    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect()
    }

    pub fn resources(&self) -> Option<&Resources> {
        self.resources.as_ref()
    }

    pub fn ws_port(&self) -> Option<u16> {
        self.ws_port
    }

    pub fn rpc_port(&self) -> Option<u16> {
        self.rpc_port
    }

    pub fn prometheus_port(&self) -> Option<u16> {
        self.prometheus_port
    }

    pub fn p2p_port(&self) -> Option<u16> {
        self.p2p_port
    }

    pub fn p2p_cert_hash(&self) -> Option<&str> {
        self.p2p_cert_hash.as_deref()
    }

    pub fn db_snapshot(&self) -> Option<&AssetLocation> {
        self.db_snapshot.as_ref()
    }
}

states! {
    Initial,
    Buildable
}

#[derive(Debug)]
pub struct NodeConfigBuilder<State> {
    config: NodeConfig,
    errors: Vec<String>,
    _state: PhantomData<State>,
}

impl Default for NodeConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: NodeConfig {
                name: "".into(),
                image: None,
                command: None,
                args: vec![],
                is_validator: false,
                is_invulnerable: false,
                is_bootnode: false,
                initial_balance: 2_000_000_000_000,
                env: vec![],
                bootnodes_addresses: vec![],
                resources: None,
                ws_port: None,
                rpc_port: None,
                prometheus_port: None,
                p2p_port: None,
                p2p_cert_hash: None,
                db_snapshot: None,
            },
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> NodeConfigBuilder<A> {
    fn transition<B>(config: NodeConfig, errors: Vec<String>) -> NodeConfigBuilder<B> {
        NodeConfigBuilder {
            config,
            errors,
            _state: PhantomData,
        }
    }
}

impl NodeConfigBuilder<Initial> {
    pub fn new(default_command: Option<String>) -> Self {
        Self::transition(
            NodeConfig {
                command: default_command,
                ..Self::default().config
            },
            vec![],
        )
    }

    pub fn with_name(self, name: impl Into<String>) -> NodeConfigBuilder<Buildable> {
        Self::transition(
            NodeConfig {
                name: name.into(),
                ..self.config
            },
            self.errors,
        )
    }
}

impl NodeConfigBuilder<Buildable> {
    pub fn with_command(self, command: impl Into<String>) -> Self {
        Self::transition(
            NodeConfig {
                command: Some(command.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_image(self, image: impl Into<String>) -> Self {
        Self::transition(
            NodeConfig {
                image: Some(image.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            NodeConfig {
                args,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn validator(self, choice: bool) -> Self {
        Self::transition(
            NodeConfig {
                is_validator: choice,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn invulnerable(self, choice: bool) -> Self {
        Self::transition(
            NodeConfig {
                is_invulnerable: choice,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn bootnode(self, choice: bool) -> Self {
        Self::transition(
            NodeConfig {
                is_bootnode: choice,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self::transition(
            NodeConfig {
                initial_balance,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_env(self, env: Vec<impl Into<EnvVar>>) -> Self {
        let env = env.into_iter().map(|var| var.into()).collect::<Vec<_>>();

        Self::transition(NodeConfig { env, ..self.config }, self.errors)
    }

    pub fn with_bootnodes_addresses(
        self,
        bootnodes_addresses: Vec<impl TryInto<Multiaddr>>,
    ) -> Self {
        let mut addrs = vec![];
        let mut errors = vec![];

        for addr in bootnodes_addresses {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(_) => errors.push("error multiaddr".to_string()),
            }
        }

        Self::transition(
            NodeConfig {
                bootnodes_addresses: addrs,
                ..self.config
            },
            vec![self.errors, errors].concat(),
        )
    }

    pub fn with_resources(self, f: fn(ResourcesBuilder) -> ResourcesBuilder) -> Self {
        let resources = Some(f(ResourcesBuilder::new()).build());

        Self::transition(
            NodeConfig {
                resources,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_ws_port(self, ws_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                ws_port: Some(ws_port),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_rpc_port(self, rpc_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                rpc_port: Some(rpc_port),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_prometheus_port(self, prometheus_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                prometheus_port: Some(prometheus_port),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_p2p_port(self, p2p_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                p2p_port: Some(p2p_port),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_p2p_cert_hash(self, p2p_cert_hash: impl Into<String>) -> Self {
        Self::transition(
            NodeConfig {
                p2p_cert_hash: Some(p2p_cert_hash.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_db_snapshot(self, location: impl TryInto<AssetLocation>) -> Self {
        match location.try_into() {
            Ok(location) => Self::transition(
                NodeConfig {
                    db_snapshot: Some(location),
                    ..self.config
                },
                self.errors,
            ),
            Err(_) => Self::transition(
                NodeConfig {
                    db_snapshot: None,
                    ..self.config
                },
                vec![
                    self.errors,
                    vec![format!("db_snapshot: error asset location",)],
                ]
                .concat(),
            ),
        }
    }

    pub fn build(self) -> NodeConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_config_builder_should_build_a_new_node_config_correctly() {
        let node_config = NodeConfigBuilder::new(None)
            .with_name("node")
            .with_command("mycommand")
            .with_image("myrepo:myimage")
            .with_args(vec![("--arg1", "value1").into(), "--option2".into()])
            .validator(true)
            .invulnerable(true)
            .bootnode(true)
            .with_initial_balance(100_000_042)
            .with_env(vec![("VAR1", "VALUE1"), ("VAR2", "VALUE2")])
            .with_bootnodes_addresses(vec![
                "/ip4/10.41.122.55/tcp/45421",
                "/ip4/51.144.222.10/tcp/2333",
            ])
            .with_resources(|resources| {
                resources
                    .with_request_cpu("200M")
                    .with_request_memory("500M")
                    .with_limit_cpu("1G")
                    .with_limit_memory("2G")
            })
            .with_ws_port(5000)
            .with_rpc_port(6000)
            .with_prometheus_port(7000)
            .with_p2p_port(8000)
            .with_p2p_cert_hash("ec8d6467180a4b72a52b24c53aa1e53b76c05602fa96f5d0961bf720edda267f")
            .with_db_snapshot("/tmp/mysnapshot")
            .build();

        assert_eq!(node_config.name(), "node");
        assert_eq!(node_config.command().unwrap(), "mycommand");
        assert_eq!(node_config.image().unwrap(), "myrepo:myimage");
        let args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        assert_eq!(node_config.args(), args.iter().collect::<Vec<_>>());
        assert!(node_config.is_validator());
        assert!(node_config.is_invulnerable());
        assert!(node_config.is_bootnode());
        assert_eq!(node_config.initial_balance(), 100_000_042);
        let env: Vec<EnvVar> = vec![("VAR1", "VALUE1").into(), ("VAR2", "VALUE2").into()];
        assert_eq!(node_config.env(), env.iter().collect::<Vec<_>>());
        let bootnodes_addresses: Vec<Multiaddr> = vec![
            "/ip4/10.41.122.55/tcp/45421".try_into().unwrap(),
            "/ip4/51.144.222.10/tcp/2333".try_into().unwrap(),
        ];
        assert_eq!(
            node_config.bootnodes_addresses(),
            bootnodes_addresses.iter().collect::<Vec<_>>()
        );
        let resources = node_config.resources().unwrap();
        assert_eq!(resources.request_cpu().unwrap().value(), "200M");
        assert_eq!(resources.request_memory().unwrap().value(), "500M");
        assert_eq!(resources.limit_cpu().unwrap().value(), "1G");
        assert_eq!(resources.limit_memory().unwrap().value(), "2G");
        assert_eq!(node_config.ws_port().unwrap(), 5000);
        assert_eq!(node_config.rpc_port().unwrap(), 6000);
        assert_eq!(node_config.prometheus_port().unwrap(), 7000);
        assert_eq!(node_config.p2p_port().unwrap(), 8000);
        assert_eq!(
            node_config.p2p_cert_hash().unwrap(),
            "ec8d6467180a4b72a52b24c53aa1e53b76c05602fa96f5d0961bf720edda267f"
        );
        assert_eq!(
            node_config
                .db_snapshot()
                .unwrap()
                .as_path_buf()
                .unwrap()
                .to_str()
                .unwrap(),
            "/tmp/mysnapshot"
        );
    }
}
