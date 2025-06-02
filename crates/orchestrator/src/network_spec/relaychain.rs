use std::collections::HashMap;

use configuration::{
    shared::{
        resources::Resources,
        types::{Arg, AssetLocation, Chain, Command, Image},
    },
    RelaychainConfig,
};
use serde::{Deserialize, Serialize};
use support::replacer::apply_replacements;

use super::node::NodeSpec;
use crate::{
    errors::OrchestratorError,
    generators::chain_spec::{ChainSpec, Context},
    shared::{constants::DEFAULT_CHAIN_SPEC_TPL_COMMAND, types::ChainDefaultContext},
};

/// A relaychain configuration spec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelaychainSpec {
    /// Chain to use (e.g. rococo-local).
    pub(crate) chain: Chain,

    /// Default command to run the node. Can be overridden on each node.
    pub(crate) default_command: Option<Command>,

    /// Default image to use (only podman/k8s). Can be overridden on each node.
    pub(crate) default_image: Option<Image>,

    /// Default resources. Can be overridden on each node.
    pub(crate) default_resources: Option<Resources>,

    /// Default database snapshot. Can be overridden on each node.
    pub(crate) default_db_snapshot: Option<AssetLocation>,

    /// Default arguments to use in nodes. Can be overridden on each node.
    pub(crate) default_args: Vec<Arg>,

    // chain_spec_path: Option<AssetLocation>,
    pub(crate) chain_spec: ChainSpec,

    /// Set the count of nominators to generator (used with PoS networks).
    pub(crate) random_nominators_count: u32,

    /// Set the max nominators value (used with PoS networks).
    pub(crate) max_nominations: u8,

    /// Genesis overrides as JSON value.
    pub(crate) runtime_genesis_patch: Option<serde_json::Value>,

    /// Wasm override path/url to use.
    pub(crate) wasm_override: Option<AssetLocation>,

    /// Nodes to run.
    pub(crate) nodes: Vec<NodeSpec>,
}

impl RelaychainSpec {
    pub fn from_config(config: &RelaychainConfig) -> Result<RelaychainSpec, OrchestratorError> {
        // Relaychain main command to use, in order:
        // set as `default_command` or
        // use the command of the first node.
        // If non of those is set, return an error.
        let main_cmd = config
            .default_command()
            .or(config.nodes().first().and_then(|node| node.command()))
            .ok_or(OrchestratorError::InvalidConfig(
                "Relaychain, either default_command or first node with a command needs to be set."
                    .to_string(),
            ))?;

        // TODO: internally we use image as String
        let main_image = config
            .default_image()
            .or(config.nodes().first().and_then(|node| node.image()))
            .map(|image| image.as_str().to_string());

        let replacements = HashMap::from([
            ("disableBootnodes", "--disable-default-bootnode"),
            ("mainCommand", main_cmd.as_str()),
        ]);
        let tmpl = if let Some(tmpl) = config.chain_spec_command() {
            apply_replacements(tmpl, &replacements)
        } else {
            apply_replacements(DEFAULT_CHAIN_SPEC_TPL_COMMAND, &replacements)
        };

        let chain_spec = ChainSpec::new(config.chain().as_str(), Context::Relay)
            .set_chain_name(config.chain().as_str())
            .command(tmpl.as_str(), config.chain_spec_command_is_local())
            .image(main_image.clone());

        // Add asset location if present
        let chain_spec = if let Some(chain_spec_path) = config.chain_spec_path() {
            chain_spec.asset_location(chain_spec_path.clone())
        } else {
            chain_spec
        };

        // build the `node_specs`
        let chain_context = ChainDefaultContext {
            default_command: config.default_command(),
            default_image: config.default_image(),
            default_resources: config.default_resources(),
            default_db_snapshot: config.default_db_snapshot(),
            default_args: config.default_args(),
        };

        let (nodes, mut errs) = config
            .nodes()
            .iter()
            .map(|node_config| NodeSpec::from_config(node_config, &chain_context))
            .fold((vec![], vec![]), |(mut nodes, mut errs), result| {
                match result {
                    Ok(node) => nodes.push(node),
                    Err(err) => errs.push(err),
                }
                (nodes, errs)
            });

        if !errs.is_empty() {
            // TODO: merge errs, maybe return something like Result<Sometype, Vec<OrchestratorError>>
            return Err(errs.swap_remove(0));
        }

        Ok(RelaychainSpec {
            chain: config.chain().clone(),
            default_command: config.default_command().cloned(),
            default_image: config.default_image().cloned(),
            default_resources: config.default_resources().cloned(),
            default_db_snapshot: config.default_db_snapshot().cloned(),
            wasm_override: config.wasm_override().cloned(),
            default_args: config.default_args().into_iter().cloned().collect(),
            chain_spec,
            random_nominators_count: config.random_nominators_count().unwrap_or(0),
            max_nominations: config.max_nominations().unwrap_or(24),
            runtime_genesis_patch: config.runtime_genesis_patch().cloned(),
            nodes,
        })
    }

    pub fn chain_spec(&self) -> &ChainSpec {
        &self.chain_spec
    }

    pub fn chain_spec_mut(&mut self) -> &mut ChainSpec {
        &mut self.chain_spec
    }
}
