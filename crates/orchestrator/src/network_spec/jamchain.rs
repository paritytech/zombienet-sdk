use std::collections::HashSet;

use configuration::{
    shared::{
        helpers::generate_unique_node_name_from_names,
        node::JamNodeConfig,
        resources::Resources,
        types::{Arg, AssetLocation, Chain, Command, Image},
    },
    types::JamProtocolParameterType,
    JamGenesisConfig, JamchainConfig,
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::OrchestratorError, network_spec::jamnode::JamNodeSpec,
    shared::types::ChainDefaultContext,
};

/// How the chain spec of a JAM chain is produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JamChainSpecSource {
    /// A pre-existing JIP-4 spec, used as given.
    Path(AssetLocation),
    /// Built in process by `jam-chainspec`. The default.
    JamBuilder(Box<JamGenesisConfig>),
}

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

    /// Protocol parameters preset.
    pub(crate) protocol_params_type: JamProtocolParameterType,

    /// How the chain spec is produced.
    pub chain_spec_source: JamChainSpecSource,

    /// Nodes to run.
    pub(crate) nodes: Vec<JamNodeSpec>,
}

impl JamchainSpec {
    pub fn from_config(config: &JamchainConfig) -> Result<JamchainSpec, OrchestratorError> {
        // The nodes need a command: set as `default_command` or on the first node.
        if config.default_command().is_none()
            && config
                .nodes()
                .first()
                .and_then(|node| node.command())
                .is_none()
        {
            return Err(OrchestratorError::InvalidConfig(
                "Jamchain, either default_command or first node with a command needs to be set."
                    .to_string(),
            ));
        }

        let chain_spec_source = Self::chain_spec_source(config)?;

        // TODO: support podman/docker/k8s
        // let main_image = config
        //     .default_image()
        //     .or(config.nodes().first().and_then(|node| node.image()))
        //     .map(|image| image.as_str().to_string());

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
            protocol_params_type: config.protocol_params_type().clone(),
            chain_spec_source,
            nodes,
        })
    }

    /// Pick how the spec is produced: a given spec, or else the built-in builder.
    fn chain_spec_source(config: &JamchainConfig) -> Result<JamChainSpecSource, OrchestratorError> {
        if let Some(path) = config.chain_spec_path() {
            // The configuration crate refuses this too, this keeps hand-built configs honest.
            if config.genesis().is_some() {
                return Err(OrchestratorError::InvalidConfig(
                    "Jamchain, `chain_spec_path` can't be used with `genesis`.".to_string(),
                ));
            }
            return Ok(JamChainSpecSource::Path(path.clone()));
        }

        Ok(JamChainSpecSource::JamBuilder(Box::new(
            config.genesis().cloned().unwrap_or_default(),
        )))
    }

    // pub fn chain_spec(&self) -> &ChainSpec {
    //     &self.chain_spec
    // }

    // pub fn chain_spec_mut(&mut self) -> &mut ChainSpec {
    //     &mut self.chain_spec
    // }
}

#[cfg(test)]
mod tests {
    use configuration::JamchainConfigBuilder;

    use super::*;

    /// With nothing named, zombienet builds the spec itself, with the default genesis.
    #[test]
    fn the_spec_is_built_in_process_by_default() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_default_command("/bin/node-binary")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        let spec = JamchainSpec::from_config(&config).expect("the spec builds");

        assert_eq!(
            spec.chain_spec_source,
            JamChainSpecSource::JamBuilder(Box::default())
        );
    }

    #[test]
    fn a_genesis_customizes_the_built_in_builder() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_bootstrap_service("/blobs/bootstrap.jam")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        let spec = JamchainSpec::from_config(&config).expect("the spec builds");

        let JamChainSpecSource::JamBuilder(genesis) = spec.chain_spec_source else {
            panic!("expected the built-in builder");
        };
        assert_eq!(
            genesis.bootstrap_service(),
            Some(&AssetLocation::from("/blobs/bootstrap.jam"))
        );
    }

    #[test]
    fn a_chain_spec_path_is_used_as_given() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_chain_spec_path("/specs/jam.json")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        let spec = JamchainSpec::from_config(&config).expect("the spec builds");

        assert_eq!(
            spec.chain_spec_source,
            JamChainSpecSource::Path("/specs/jam.json".into())
        );
    }
}
