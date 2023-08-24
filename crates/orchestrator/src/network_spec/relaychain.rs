use configuration::{
    shared::{
        resources::Resources,
        types::{Arg, AssetLocation, Chain, Command, Image},
    },
    RelaychainConfig,
};

use super::node::{ChainDefaultContext, NodeSpec};
use crate::{chain_spec::ChainSpec, errors::OrchestratorError};

/// A relaychain configuration spec
#[derive(Debug, Clone)]
pub struct RelaychainSpec {
    /// Chain to use (e.g. rococo-local).
    chain: Chain,

    // /// Default command to run the node. Can be overriden on each node.
    // default_command: Command,

    // /// Default image to use (only podman/k8s). Can be overriden on each node.
    // default_image: Option<Image>,

    // /// Default resources. Can be overriden on each node.
    // default_resources: Option<Resources>,

    // /// Default database snapshot. Can be overriden on each node.
    // default_db_snapshot: Option<AssetLocation>,

    // /// Default arguments to use in nodes. Can be overriden on each node.
    // default_args: Vec<Arg>,

    // /// Command to build the plain chain spec
    // chain_spec_command: Option<String>,

    // /// Chain specification JSON file to use. If is provider we will `not` create the spec
    // /// with `chain_spec_command`
    // chain_spec_path: Option<AssetLocation>,
    chain_spec: ChainSpec,

    /// Set the count of nominators to generator (used with PoS networks).
    random_nominators_count: u32,

    /// Set the max nominators value (used with PoS networks).
    max_nominations: u8,

    /// Nodes to run.
    nodes: Vec<NodeSpec>,
}

impl RelaychainSpec {
    pub fn from_config(config: &RelaychainConfig) -> Result<RelaychainSpec, OrchestratorError> {
        // Relaychain main command to use, in order:
        // set as `default_command` or
        // use the command of the first node.
        // If non of those is set, return an error.
        let main_cmd = if let Some(cmd) = config.default_command() {
            cmd
        } else {
            if let Some(first_node) = config.nodes().first() {
                let Some(cmd) = first_node.command() else {
                    return Err(OrchestratorError::InvalidConfig("Relaychain, either default_command or command in the first node needs to be set.".to_string()));
                };

                cmd
            } else {
                return Err(OrchestratorError::InvalidConfig(
                    "Relaychain without nodes and default_command isn't set.".to_string(),
                ));
            }
        };

        let chain_spec = if let Some(chain_spec_path) = config.chain_spec_path() {
            ChainSpec::new_with_path(config.chain().as_str(), chain_spec_path.to_string())
        } else {
            // TODO: Do we need to add the posibility to set the command to use?
            // Currently (v1) is possible but when is set is set to the default command.
            ChainSpec::new(config.chain().as_str(), main_cmd.as_str())
        };

        // build the `node_specs`
        let chain_context = ChainDefaultContext {
            default_command: config.default_command(),
            default_image: config.default_image(),
            default_resources: config.default_resources(),
            default_db_snapshot: config.default_db_snapshot(),
            default_args: config.default_args(),
        };

        // We want to track the errors for all the nodes and report them ones
        let mut errs: Vec<OrchestratorError> = Default::default();

        let mut nodes: Vec<NodeSpec> = Default::default();
        config.nodes().iter().for_each(|node_config| {
            match NodeSpec::from_config(&node_config, &chain_context) {
                Ok(node) => nodes.push(node),
                Err(err) => errs.push(err),
            }
        });

        if !errs.is_empty() {
            // TODO: merge errs
            return Err(errs.swap_remove(0));
        }

        Ok(RelaychainSpec {
            chain: config.chain().clone(),
            // default_command: todo!(),
            // default_image: todo!(),
            // default_resources: todo!(),
            // default_db_snapshot: todo!(),
            // default_args: todo!(),
            chain_spec,
            random_nominators_count: config.random_nominators_count().unwrap_or(0),
            max_nominations: config.max_nominations().unwrap_or(24),
            nodes,
        })
    }
}
