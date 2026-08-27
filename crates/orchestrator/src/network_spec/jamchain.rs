use std::collections::{HashMap, HashSet};

use configuration::{
    shared::{
        helpers::generate_unique_node_name_from_names,
        node::JamNodeConfig,
        resources::Resources,
        types::{Arg, Chain, Command, Image},
    },
    JamchainConfig,
};
use serde::{Deserialize, Serialize};
use support::replacer::apply_replacements;

use crate::{
    errors::OrchestratorError,
    network_spec::jamnode::JamNodeSpec,
    shared::{constants::DEFAULT_JAM_CHAIN_SPEC_TPL_COMMAND, types::ChainDefaultContext},
};

/// A relaychain configuration spec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JamchainSpec {
    /// Id to use (e.g. dev).
    pub(crate) id: Chain,

    /// Default command to run the node. Can be overridden on each node.
    pub(crate) default_command: Option<Command>,

    /// Default image to use (only podman/k8s). Can be overridden on each node.
    pub(crate) default_image: Option<Image>,

    /// Default resources. Can be overridden on each node.
    pub(crate) default_resources: Option<Resources>,

    /// Default arguments to use in nodes. Can be overridden on each node.
    pub(crate) default_args: Vec<Arg>,

    // chain_spec_path: Option<AssetLocation>,
    // pub(crate) chain_spec: ChainSpec,
    /// Chain-spec generator resolved
    pub chain_spec_command: String,

    /// Nodes to run.
    pub(crate) nodes: Vec<JamNodeSpec>,
}

impl JamchainSpec {
    pub fn from_config(config: &JamchainConfig) -> Result<JamchainSpec, OrchestratorError> {
        // main command to use, in order:
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

        // TODO: support podman/docker/k8s
        // let main_image = config
        //     .default_image()
        //     .or(config.nodes().first().and_then(|node| node.image()))
        //     .map(|image| image.as_str().to_string());

        let replacements = HashMap::from([
            ("mainCommand", main_cmd.as_str()),
            ("subCommand", "gen-spec"),
        ]);

        let chain_spec_cmd_augmented = if let Some(tmpl) = config.chain_spec_command() {
            apply_replacements(tmpl.as_str(), &replacements)
        } else {
            apply_replacements(DEFAULT_JAM_CHAIN_SPEC_TPL_COMMAND, &replacements)
        };

        // TODO: handle chain-spec build/customization

        // build the `node_specs`
        let chain_context = ChainDefaultContext {
            default_command: config.default_command(),
            default_image: config.default_image(),
            default_resources: config.default_resources(),
            default_db_snapshot: None,
            default_args: config.default_args(),
        };

        let nodes: Vec<JamNodeConfig> = config.nodes().into_iter().cloned().collect();
        // nodes.extend(
        //     config
        //         .group_node_configs()
        //         .into_iter()
        //         .flat_map(|node_group| node_group.expand_group_configs()),
        // );

        let mut names = HashSet::new();
        let (nodes, mut errs) = nodes
            .iter()
            .map(|node_config| JamNodeSpec::from_config(node_config, &chain_context))
            .fold((vec![], vec![]), |(mut nodes, mut errs), result| {
                match result {
                    Ok(mut node) => {
                        let unique_name =
                            generate_unique_node_name_from_names(node.name, &mut names);
                        node.name = unique_name;
                        nodes.push(node);
                    },
                    Err(err) => errs.push(err),
                }
                (nodes, errs)
            });

        if !errs.is_empty() {
            // TODO: merge errs, maybe return something like Result<Sometype, Vec<OrchestratorError>>
            return Err(errs.swap_remove(0));
        }

        Ok(JamchainSpec {
            id: config.id().clone(),
            default_command: config.default_command().cloned(),
            default_image: config.default_image().cloned(),
            default_resources: config.default_resources().cloned(),
            default_args: config.default_args().into_iter().cloned().collect(),
            chain_spec_command: chain_spec_cmd_augmented,
            nodes,
        })
    }

    // pub fn chain_spec(&self) -> &ChainSpec {
    //     &self.chain_spec
    // }

    // pub fn chain_spec_mut(&mut self) -> &mut ChainSpec {
    //     &mut self.chain_spec
    // }
}
