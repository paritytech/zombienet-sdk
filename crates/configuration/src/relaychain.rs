use crate::shared::{
    node::NodeConfig,
    types::{Arg, Command, ContainerImage, DbSnapshot, Resources},
};

/// A relaychain configuration, composed of nodes and fine-grained configuration options.
#[derive(Debug, Clone)]
pub struct RelaychainConfig {
    /// Default command to run the node. Can be overriden on each node.
    default_command: Option<Command>,

    /// Default image to use (only podman/k8s). Can be overriden on each node.
    default_image: Option<ContainerImage>,

    /// Default resources. Can be overriden on each node.
    default_resources: Option<Resources>,

    /// Default database snapshot. Can be overriden on each node.
    default_db_snapshot: Option<DbSnapshot>,

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
