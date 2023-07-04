use std::{error::Error, fmt::Display, marker::PhantomData};

use multiaddr::Multiaddr;

use super::{
    errors::FieldError,
    helpers::{merge_errors, merge_errors_vecs},
    macros::states,
    resources::ResourcesBuilder,
    types::{AssetLocation, ChainDefaultContext, Command, Image},
};
use crate::shared::{
    resources::Resources,
    types::{Arg, Port},
};

/// An environment variable with a name and a value.
/// It can be constructed from a `(&str, &str)`.
///
/// # Examples:
///
/// ```
/// use configuration::shared::node::EnvVar;
/// 
/// let simple_var: EnvVar = ("FOO", "BAR").into();
///
/// assert_eq!(
///     simple_var,
///     EnvVar {
///         name: "FOO".into(),
///         value: "BAR".into()
///     }
/// )
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct EnvVar {
    /// The name of the environment variable.
    pub name: String,

    /// The value of the environment variable.
    pub value: String,
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
    name: String,
    image: Option<Image>,
    command: Option<Command>,
    args: Vec<Arg>,
    is_validator: bool,
    is_invulnerable: bool,
    is_bootnode: bool,
    initial_balance: u128,
    env: Vec<EnvVar>,
    bootnodes_addresses: Vec<Multiaddr>,
    resources: Option<Resources>,
    ws_port: Option<Port>,
    rpc_port: Option<Port>,
    prometheus_port: Option<Port>,
    p2p_port: Option<Port>,
    p2p_cert_hash: Option<String>,
    db_snapshot: Option<AssetLocation>,
}

impl NodeConfig {
    /// Node name (should be unique).
    // TODO: handle unique in validation
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Image to run (only podman/k8s). Override the default.
    // TODO: missing test of default overridding
    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    /// Command to run the node. Override the default.
    // TODO: missing test of default overriding
    pub fn command(&self) -> Option<&Command> {
        self.command.as_ref()
    }

    /// Arguments to use for node. Appended to default.
    // TODO: missing test of default appending
    pub fn args(&self) -> Vec<&Arg> {
        self.args.iter().collect()
    }

    /// Whether the node is a validator.
    pub fn is_validator(&self) -> bool {
        self.is_validator
    }

    /// Whether the node keys must be added to invulnerables.
    pub fn is_invulnerable(&self) -> bool {
        self.is_invulnerable
    }

    /// Whether the node is a bootnode.
    pub fn is_bootnode(&self) -> bool {
        self.is_bootnode
    }

    /// Node initial balance present in genesis.
    pub fn initial_balance(&self) -> u128 {
        self.initial_balance
    }

    /// Environment variables to set (inside pod for podman/k8s, inside shell for native).
    pub fn env(&self) -> Vec<&EnvVar> {
        self.env.iter().collect()
    }

    /// List of node's bootnodes addresses to use. Appended to default.
    // TODO: missing test appending to default.
    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect()
    }

    /// Default resources. Override the default.
    // TODO: missing test default override.
    pub fn resources(&self) -> Option<&Resources> {
        self.resources.as_ref()
    }

    /// Websocket port to use. Default to 9944 + n where n is the node index in the network (starting from 0).
    // TODO: needs to test the default + incrementation
    pub fn ws_port(&self) -> Option<u16> {
        self.ws_port
    }

    /// RPC port to use. Default to 9933 + n where n is the node index in the network (starting from 0).
    // [TODO]: start at a different default to avoid overlap between ws_port and rpc_port when node count >= 12 ? + testing
    pub fn rpc_port(&self) -> Option<u16> {
        self.rpc_port
    }

    /// Prometheus port to use. Default to 9615 + n where n is the node index in the network (starting from 0).
    // TODO: needs to tes the defualt + incrementation
    pub fn prometheus_port(&self) -> Option<u16> {
        self.prometheus_port
    }

    /// P2P port to use. Default to 30333 + n where n is the node index in the network (starting from 0)
    // TODO: needs to tes the defualt + incrementation
    pub fn p2p_port(&self) -> Option<u16> {
        self.p2p_port
    }

    /// `libp2p` cert hash to use with `WebRTC` transport.
    pub fn p2p_cert_hash(&self) -> Option<&str> {
        self.p2p_cert_hash.as_deref()
    }

    /// Database snapshot. Override the default.
    // TODO: missing override test
    pub fn db_snapshot(&self) -> Option<&AssetLocation> {
        self.db_snapshot.as_ref()
    }
}

states! {
    Initial,
    Buildable
}

/// A node configuration builder, used to build a [`NodeConfig`] declaratively with fields validation.
#[derive(Debug)]
pub struct NodeConfigBuilder<S> {
    config: NodeConfig,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<S>,
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
    fn transition<B>(config: NodeConfig, errors: Vec<anyhow::Error>) -> NodeConfigBuilder<B> {
        NodeConfigBuilder {
            config,
            errors,
            _state: PhantomData,
        }
    }
}

impl NodeConfigBuilder<Initial> {
    pub fn new(context: ChainDefaultContext) -> Self {
        Self::transition(
            NodeConfig {
                command: context.default_command,
                image: context.default_image,
                resources: context.default_resources,
                db_snapshot: context.default_db_snapshot,
                args: context.default_args,
                ..Self::default().config
            },
            vec![],
        )
    }

    /// Set the name of the node.
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
    /// Set the command that will be executed to launch the node.
    pub fn with_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                NodeConfig {
                    command: Some(command),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::Command(error.into()).into()),
            ),
        }
    }

    /// Set the image that will be used for the node (only podman/k8s).
    pub fn with_image<T>(self, image: T) -> Self
    where
        T: TryInto<Image>,
        T::Error: Error + Send + Sync + 'static,
    {
        match image.try_into() {
            Ok(image) => Self::transition(
                NodeConfig {
                    image: Some(image),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::Image(error.into()).into()),
            ),
        }
    }

    /// Set the arguments that will be used when launching the node.
    pub fn with_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            NodeConfig {
                args,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set whether the node is a validator.
    pub fn validator(self, choice: bool) -> Self {
        Self::transition(
            NodeConfig {
                is_validator: choice,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set whether the node is invulnerable.
    pub fn invulnerable(self, choice: bool) -> Self {
        Self::transition(
            NodeConfig {
                is_invulnerable: choice,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set whether the node is a bootnode.
    pub fn bootnode(self, choice: bool) -> Self {
        Self::transition(
            NodeConfig {
                is_bootnode: choice,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the node initial balance.
    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self::transition(
            NodeConfig {
                initial_balance,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the node environment variables that will be used when launched.
    pub fn with_env(self, env: Vec<impl Into<EnvVar>>) -> Self {
        let env = env.into_iter().map(|var| var.into()).collect::<Vec<_>>();

        Self::transition(NodeConfig { env, ..self.config }, self.errors)
    }

    /// Set the bootnodes addresses that the node will try to connect to.
    pub fn with_bootnodes_addresses<T>(self, bootnodes_addresses: Vec<T>) -> Self
    where
        T: TryInto<Multiaddr> + Display + Copy,
        T::Error: Error + Send + Sync + 'static,
    {
        let mut addrs = vec![];
        let mut errors = vec![];

        for (index, addr) in bootnodes_addresses.into_iter().enumerate() {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(error) => errors.push(
                    FieldError::BootnodesAddress(index, addr.to_string(), error.into()).into(),
                ),
            }
        }

        Self::transition(
            NodeConfig {
                bootnodes_addresses: addrs,
                ..self.config
            },
            merge_errors_vecs(self.errors, errors),
        )
    }

    /// Set the resources limits what will be used for the node (only podman/k8s).
    pub fn with_resources(self, f: fn(ResourcesBuilder) -> ResourcesBuilder) -> Self {
        match f(ResourcesBuilder::new()).build() {
            Ok(resources) => Self::transition(
                NodeConfig {
                    resources: Some(resources),
                    ..self.config
                },
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| FieldError::Resources(error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Set the websocket port that will be exposed.
    pub fn with_ws_port(self, ws_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                ws_port: Some(ws_port),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the RPC port that will be exposed.
    pub fn with_rpc_port(self, rpc_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                rpc_port: Some(rpc_port),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the prometheus port that will be exposed for metrics.
    pub fn with_prometheus_port(self, prometheus_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                prometheus_port: Some(prometheus_port),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the P2P port that will be exposed.
    pub fn with_p2p_port(self, p2p_port: Port) -> Self {
        Self::transition(
            NodeConfig {
                p2p_port: Some(p2p_port),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the P2P cert hash that will be used
    // TODO: for what it can be used ?
    pub fn with_p2p_cert_hash(self, p2p_cert_hash: impl Into<String>) -> Self {
        Self::transition(
            NodeConfig {
                p2p_cert_hash: Some(p2p_cert_hash.into()),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the database snapshot that will be used to launch the node.
    pub fn with_db_snapshot(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            NodeConfig {
                db_snapshot: Some(location.into()),
                ..self.config
            },
            self.errors,
        )
    }

    /// Seals the builder and returns a [`NodeConfig`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<NodeConfig, (String, Vec<anyhow::Error>)> {
        if !self.errors.is_empty() {
            return Err((self.config.name.clone(), self.errors));
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_config_builder_should_succeeds_and_returns_a_node_config() {
        let node_config = NodeConfigBuilder::new(ChainDefaultContext::default())
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
            .build()
            .unwrap();

        assert_eq!(node_config.name(), "node");
        assert_eq!(node_config.command().unwrap().as_str(), "mycommand");
        assert_eq!(node_config.image().unwrap().as_str(), "myrepo:myimage");
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
        assert_eq!(resources.request_cpu().unwrap().as_str(), "200M");
        assert_eq!(resources.request_memory().unwrap().as_str(), "500M");
        assert_eq!(resources.limit_cpu().unwrap().as_str(), "1G");
        assert_eq!(resources.limit_memory().unwrap().as_str(), "2G");
        assert_eq!(node_config.ws_port().unwrap(), 5000);
        assert_eq!(node_config.rpc_port().unwrap(), 6000);
        assert_eq!(node_config.prometheus_port().unwrap(), 7000);
        assert_eq!(node_config.p2p_port().unwrap(), 8000);
        assert_eq!(
            node_config.p2p_cert_hash().unwrap(),
            "ec8d6467180a4b72a52b24c53aa1e53b76c05602fa96f5d0961bf720edda267f"
        );
        assert!(matches!(
            node_config.db_snapshot().unwrap(), AssetLocation::FilePath(value) if value.to_str().unwrap() == "/tmp/mysnapshot"
        ));
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_an_error_and_node_name_if_command_is_invalid() {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_command("invalid command")
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_an_error_and_node_name_if_image_is_invalid() {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_image("myinvalid.image")
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "image: 'myinvalid.image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_an_error_and_node_name_if_one_bootnode_address_is_invalid(
    ) {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421"])
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_mulitle_errors_and_node_name_if_multiple_bootnode_address_are_invalid(
    ) {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421", "//10.42.153.10/tcp/43111"])
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "bootnodes_addresses[1]: '//10.42.153.10/tcp/43111' unknown protocol string: "
        );
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_an_error_and_node_name_if_resources_has_an_error(
    ) {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_resources(|resources| resources.with_limit_cpu("invalid"))
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            r"resources.limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_multiple_errors_and_node_name_if_resources_has_multiple_errors(
    ) {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_resources(|resources| {
                resources
                    .with_limit_cpu("invalid")
                    .with_request_memory("invalid")
            })
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            r"resources.limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            r"resources.request_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn node_config_builder_should_fails_and_returns_multiple_errors_and_node_name_if_multiple_fields_have_errors(
    ) {
        let (node_name, errors) = NodeConfigBuilder::new(ChainDefaultContext::default())
            .with_name("node")
            .with_command("invalid command")
            .with_image("myinvalid.image")
            .with_resources(|resources| {
                resources
                    .with_limit_cpu("invalid")
                    .with_request_memory("invalid")
            })
            .build()
            .unwrap_err();

        assert_eq!(node_name, "node");
        assert_eq!(errors.len(), 4);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "command: 'invalid command' shouldn't contains whitespace"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "image: 'myinvalid.image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
        assert_eq!(
            errors.get(2).unwrap().to_string(),
            r"resources.limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
        assert_eq!(
            errors.get(3).unwrap().to_string(),
            r"resources.request_memory: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }
}
