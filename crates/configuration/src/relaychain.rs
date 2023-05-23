use crate::shared::{
    node::NodeConfig,
    types::{Arg, Command, ContainerImage, AssetLocation, Resources},
};

/// A relaychain configuration, composed of nodes and fine-grained configuration options.
pub struct RelaychainConfig {
    /// Default command to run the node. Can be overriden on each node.
    default_command: Option<Command>,

    /// Default image to use (only podman/k8s). Can be overriden on each node.
    default_image: Option<ContainerImage>,

    /// Default resources. Can be overriden on each node.
    default_resources: Option<Resources>,

    /// Default database snapshot. Can be overriden on each node.
    default_db_snapshot: Option<AssetLocation>,

    /// Chain to use (e.g. rococo-local).
    chain: String,

    /// Chain specification JSON file to use.
    chain_spec_path: Option<String>,

    /// Default arguments to use in nodes. Can be overriden on each node.
    default_args: Vec<Arg>,

    /// Set the count of nominators to generator (used with PoS networks).
    random_nominators_count: Option<u32>,

    /// Set the max nominators value (used with PoS networks).
    max_nominations: Option<u16>,

    /// Nodes to run.
    nodes: Vec<NodeConfig>,
    // [TODO]: do we need node_groups in the sdk?
    // node_groups?: NodeGroupConfig[];

    // [TODO]: allow customize genesis
    // genesis?: JSON | ObjectJSON;
}

impl Default for RelaychainConfig {
    fn default() -> Self {
        // [TODO]: define the default value for a relaychain
        todo!()
    }
}

impl RelaychainConfig {
    pub fn with_default_command(self, command: Command) -> Self {
        Self {
            default_command: Some(command),
            ..self
        }
    }

    pub fn with_default_image(self, image: ContainerImage) -> Self {
        Self {
            default_image: Some(image),
            ..self
        }
    }

    pub fn with_default_resources(self, f: fn(Resources) -> Resources) -> Self {
        Self {
            default_resources: Some(f(Resources::default())),
            ..self
        }
    }

    pub fn with_default_db_snapshot(self, location: AssetLocation) -> Self {
        Self {
            default_db_snapshot: Some(location),
            ..self
        }
    }

    pub fn with_chain(self, chain: String) -> Self {
        Self { chain, ..self }
    }

    pub fn with_chain_spec_path(self, chain_spec_path: String) -> Self {
        Self {
            chain_spec_path: Some(chain_spec_path),
            ..self
        }
    }

    pub fn with_default_args(self, args: Vec<Arg>) -> Self {
        Self {
            default_args: args,
            ..self
        }
    }

    pub fn with_random_nominators_count(self, random_nominators_count: u32) -> Self {
        Self {
            random_nominators_count: Some(random_nominators_count),
            ..self
        }
    }

    pub fn with_max_nominations(self, max_nominations: u16) -> Self {
        Self {
            max_nominations: Some(max_nominations),
            ..self
        }
    }

    pub fn with_node(self, f: fn(NodeConfig) -> NodeConfig) -> Self {
        Self {
            nodes: vec![self.nodes, vec![f(NodeConfig::default())]].concat(),
            ..self
        }
    }
}
