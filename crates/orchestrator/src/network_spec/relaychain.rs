use configuration::{
    shared::{
        resources::Resources,
        types::{Arg, AssetLocation, Chain, Command, Image},
    },
    RelaychainConfig,
};

use super::node::NodeSpec;
use crate::{
    errors::OrchestratorError, generators::chain_spec::{ChainSpec, Context},
    shared::types::ChainDefaultContext,
};

/// A relaychain configuration spec
#[derive(Debug, Clone)]
pub struct RelaychainSpec {
    /// Chain to use (e.g. rococo-local).
    pub(crate) chain: Chain,

    /// Default command to run the node. Can be overriden on each node.
    pub(crate) default_command: Option<Command>,

    /// Default image to use (only podman/k8s). Can be overriden on each node.
    pub(crate) default_image: Option<Image>,

    /// Default resources. Can be overriden on each node.
    pub(crate) default_resources: Option<Resources>,

    /// Default database snapshot. Can be overriden on each node.
    pub(crate) default_db_snapshot: Option<AssetLocation>,

    /// Default arguments to use in nodes. Can be overriden on each node.
    pub(crate) default_args: Vec<Arg>,

    // chain_spec_path: Option<AssetLocation>,
    pub(crate) chain_spec: ChainSpec,

    /// Set the count of nominators to generator (used with PoS networks).
    pub(crate) random_nominators_count: u32,

    /// Set the max nominators value (used with PoS networks).
    pub(crate) max_nominations: u8,

    /// Nodes to run.
    pub(crate) nodes: Vec<NodeSpec>,
}

impl RelaychainSpec {
    pub fn from_config(config: &RelaychainConfig) -> Result<RelaychainSpec, OrchestratorError> {
        // Relaychain main command to use, in order:
        // set as `default_command` or
        // use the command of the first node.
        // If non of those is set, return an error.
        let main_cmd = if let Some(cmd) = config.default_command() {
            cmd
        } else if let Some(first_node) = config.nodes().first() {
            let Some(cmd) = first_node.command() else {
                return Err(OrchestratorError::InvalidConfig("Relaychain, either default_command or command in the first node needs to be set.".to_string()));
            };

            cmd
        } else {
            return Err(OrchestratorError::InvalidConfig(
                "Relaychain without nodes and default_command isn't set.".to_string(),
            ));
        };

        let chain_spec = ChainSpec::new(config.chain().as_str(), Context::Relay).set_chain_name(config.chain().as_str());
        let chain_spec = if let Some(chain_spec_path) = config.chain_spec_path() {
            chain_spec.asset_location(chain_spec_path.clone())
        } else {
            // TODO: Do we need to add the posibility to set the command to use?
            // Currently (v1) is possible but when is set is set to the default command.
            chain_spec.commad(main_cmd.as_str())
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
            match NodeSpec::from_config(node_config, &chain_context) {
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
            default_command: config.default_command().cloned(),
            default_image: config.default_image().cloned(),
            default_resources: config.default_resources().cloned(),
            default_db_snapshot: config.default_db_snapshot().cloned(),
            default_args: config.default_args().into_iter().cloned().collect(),
            chain_spec,
            random_nominators_count: config.random_nominators_count().unwrap_or(0),
            max_nominations: config.max_nominations().unwrap_or(24),
            nodes,
        })
    }
}
