use crate::shared::types::{Arg, MultiAddress, Port, Resources};

use super::types::AssetLocation;

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
#[derive(Debug, Default, Clone, PartialEq)]
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

impl NodeConfig {
    pub fn with_name(self, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..self
        }
    }

    pub fn with_image(self, image: &str) -> Self {
        Self {
            image: Some(image.to_owned()),
            ..self
        }
    }

    pub fn with_command(self, command: &str) -> Self {
        Self {
            command: Some(command.to_owned()),
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

    pub fn with_p2p_cert_hash(self, p2p_cert_hash: &str) -> Self {
        Self {
            p2p_cert_hash: Some(p2p_cert_hash.to_owned()),
            ..self
        }
    }

    pub fn with_db_snapshot(self, location: AssetLocation) -> Self {
        Self {
            db_snapshot: Some(location),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn image(&self) -> Option<&str> {
        self.image.as_ref().map(|image| image.as_str())
    }

    pub fn command(&self) -> Option<&str> {
        self.command.as_ref().map(|command| command.as_str())
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

    pub fn bootnodes_addresses(&self) -> Vec<&MultiAddress> {
        self.bootnodes_addresses
            .iter()
            .collect::<Vec<&MultiAddress>>()
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
        self.p2p_cert_hash
            .as_ref()
            .map(|p2p_cert_hash| p2p_cert_hash.as_str())
    }

    pub fn db_snapshot(&self) -> Option<&AssetLocation> {
        self.db_snapshot.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_name_should_update_the_name_on_the_node_config() {
        let node_config = NodeConfig::default().with_name("a_node_name");

        assert_eq!(node_config.name(), "a_node_name");
    }

    #[test]
    fn with_image_should_update_the_image_on_the_node_config() {
        let node_config = NodeConfig::default().with_image("myrepo:myimage");

        assert_eq!(node_config.image().unwrap(), "myrepo:myimage");
    }

    #[test]
    fn with_command_should_update_the_command_on_the_node_config() {
        let node_config = NodeConfig::default().with_command("my command to run");

        assert_eq!(node_config.command().unwrap(), "my command to run");
    }

    #[test]
    fn with_args_should_update_the_args_on_the_node_config() {
        let args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        let node_config = NodeConfig::default().with_args(args.clone());

        assert_eq!(node_config.args(), args.iter().collect::<Vec<&Arg>>());
    }

    #[test]
    fn as_validator_should_update_the_is_validator_property_on_the_node_config() {
        let node_config = NodeConfig::default().as_validator();

        assert!(node_config.is_validator());
    }

    #[test]
    fn as_invulnerable_should_update_the_is_invulnerable_property_on_the_node_config() {
        let node_config = NodeConfig::default().as_invulnerable();

        assert!(node_config.is_invulnerable());
    }

    #[test]
    fn as_bootnode_should_update_the_is_bootnode_property_on_the_node_config() {
        let node_config = NodeConfig::default().as_bootnode();

        assert!(node_config.is_bootnode());
    }

    #[test]
    fn with_initial_balance_should_update_the_initial_balance_on_the_node_config() {
        let node_config = NodeConfig::default().with_initial_balance(424242424242);

        assert_eq!(node_config.initial_balance(), 424242424242);
    }

    #[test]

    fn with_env_should_update_the_env_on_the_node_config() {
        let env: Vec<EnvVar> = vec![("VAR1", "VALUE1").into(), ("VAR2", "VALUE2").into()];
        let node_config = NodeConfig::default().with_env(env.clone());

        assert_eq!(node_config.env(), env.iter().collect::<Vec<&EnvVar>>());
    }

    #[test]
    fn with_bootnodes_addresses_should_update_the_bootnodes_addresses_on_the_node_config() {
        let bootnodes_addresses = vec![
            "/ip4/10.41.122.55/tcp/45421".into(),
            "/ip4/51.144.222.10/tcp/2333".into(),
        ];
        let node_config =
            NodeConfig::default().with_bootnodes_addresses(bootnodes_addresses.clone());

        assert_eq!(
            node_config.bootnodes_addresses(),
            bootnodes_addresses.iter().collect::<Vec<&MultiAddress>>()
        );
    }

    #[test]
    fn with_resources_should_update_the_resources_on_the_node_config() {
        let node_config = NodeConfig::default().with_resources(|resources| {
            resources
                .with_request_cpu("200M")
                .with_request_memory("500M")
                .with_limit_cpu("1G")
                .with_limit_memory("2G")
        });

        assert_eq!(
            node_config
                .resources()
                .unwrap()
                .request_cpu()
                .unwrap()
                .value(),
            "200M"
        );
        assert_eq!(
            node_config
                .resources()
                .unwrap()
                .request_memory()
                .unwrap()
                .value(),
            "500M"
        );
        assert_eq!(
            node_config
                .resources()
                .unwrap()
                .limit_cpu()
                .unwrap()
                .value(),
            "1G"
        );
        assert_eq!(
            node_config
                .resources()
                .unwrap()
                .limit_memory()
                .unwrap()
                .value(),
            "2G"
        );
    }

    #[test]
    fn with_ws_port_should_update_the_ws_port_on_the_node_config() {
        let node_config = NodeConfig::default().with_ws_port(4444);

        assert_eq!(node_config.ws_port().unwrap(), 4444);
    }

    #[test]
    fn with_rpc_port_should_update_the_rpc_port_on_the_node_config() {
        let node_config = NodeConfig::default().with_rpc_port(5555);

        assert_eq!(node_config.rpc_port().unwrap(), 5555);
    }

    #[test]
    fn with_prometheus_port_should_update_the_prometheus_port_on_the_node_config() {
        let node_config = NodeConfig::default().with_prometheus_port(6666);

        assert_eq!(node_config.prometheus_port().unwrap(), 6666);
    }

    #[test]
    fn with_p2p_port_should_update_the_p2p_port_on_the_node_config() {
        let node_config = NodeConfig::default().with_p2p_port(7777);

        assert_eq!(node_config.p2p_port().unwrap(), 7777);
    }

    #[test]
    fn with_p2p_cert_hash_should_update_the_p2p_cert_hash_on_the_node_config() {
        let hash = "ec8d6467180a4b72a52b24c53aa1e53b76c05602fa96f5d0961bf720edda267f"; // sha256("myhash")
        let node_config = NodeConfig::default().with_p2p_cert_hash(hash);

        assert_eq!(node_config.p2p_cert_hash().unwrap(), hash);
    }

    #[test]
    fn with_db_snapshot_should_update_the_db_snapshot_on_the_node_config() {
        let location = AssetLocation::FilePath("/tmp/mysnapshot".into());
        let node_config = NodeConfig::default().with_db_snapshot(location.clone());

        assert_eq!(node_config.db_snapshot().unwrap(), &location);
    }
}
