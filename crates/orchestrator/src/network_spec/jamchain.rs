use std::collections::{HashMap, HashSet};

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
use support::replacer::apply_replacements;

use crate::{
    errors::OrchestratorError,
    network_spec::jamnode::JamNodeSpec,
    shared::{constants::DEFAULT_JAM_CHAIN_SPEC_TPL_COMMAND, types::ChainDefaultContext},
};

/// How the chain spec of a JAM chain is produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JamChainSpecSource {
    /// A pre-existing JIP-4 spec, used as given.
    Path(AssetLocation),
    /// An external `<cmd> gen-spec <config> <spec>`, opted into with `chain_spec_command` or
    /// `genesis_overrides`.
    Generator { command: String },
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

    /// Genesis state beyond the validator set, merged into the config `gen-spec` reads.
    pub(crate) genesis_overrides: Option<serde_json::Value>,
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

        if config
            .genesis_overrides()
            .is_some_and(|overrides| !overrides.is_object())
        {
            return Err(OrchestratorError::InvalidConfig(
                "Jamchain, genesis_overrides must be a JSON object.".to_string(),
            ));
        }

        let chain_spec_source = Self::chain_spec_source(config, main_cmd)?;

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
            genesis_overrides: config.genesis_overrides().cloned(),
        })
    }

    /// Pick how the spec is produced: a given spec, an external generator when one is asked
    /// for, or else the built-in builder.
    fn chain_spec_source(
        config: &JamchainConfig,
        main_cmd: &Command,
    ) -> Result<JamChainSpecSource, OrchestratorError> {
        let generator_field = if config.chain_spec_command().is_some() {
            Some("chain_spec_command")
        } else if config.genesis_overrides().is_some() {
            Some("genesis_overrides")
        } else {
            None
        };

        // The configuration crate refuses these too, this keeps hand-built configs honest.
        if let (Some(field), Some(_)) = (generator_field, config.genesis()) {
            return Err(OrchestratorError::InvalidConfig(format!(
                "Jamchain, `genesis` can't be used with `{field}`."
            )));
        }

        if let Some(path) = config.chain_spec_path() {
            if config.genesis().is_some() || generator_field.is_some() {
                return Err(OrchestratorError::InvalidConfig(
                    "Jamchain, `chain_spec_path` can't be used with `genesis`, \
                     `chain_spec_command` or `genesis_overrides`."
                        .to_string(),
                ));
            }
            return Ok(JamChainSpecSource::Path(path.clone()));
        }

        if generator_field.is_some() {
            let replacements = HashMap::from([
                ("mainCommand", main_cmd.as_str()),
                ("subCommand", "gen-spec"),
            ]);
            let tmpl = config
                .chain_spec_command()
                .map(|cmd| cmd.as_str())
                .unwrap_or(DEFAULT_JAM_CHAIN_SPEC_TPL_COMMAND);
            return Ok(JamChainSpecSource::Generator {
                command: apply_replacements(tmpl, &replacements),
            });
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

    /// The chain spec is not always generated by the binary that runs the nodes: the two are
    /// separate commands here, and only the first token of the template is used because the
    /// orchestrator appends `gen-spec <config> <spec>` itself.
    #[test]
    fn a_chain_spec_command_of_its_own_overrides_the_node_binary() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_default_command("/bin/node-binary")
            .with_chain_spec_command("/bin/spec-binary")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        let spec = JamchainSpec::from_config(&config).expect("the spec builds");

        assert_eq!(
            spec.chain_spec_source,
            JamChainSpecSource::Generator {
                command: "/bin/spec-binary".into()
            }
        );
    }

    /// The override is merged key by key into the generated config, so anything but an object
    /// has nothing to merge into — and silently writing the config without it would surface
    /// only as a chain missing its services.
    #[test]
    fn a_genesis_override_that_is_not_an_object_is_refused() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_default_command("/bin/node-binary")
            .with_genesis_overrides(serde_json::json!(["services"]))
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        let error = JamchainSpec::from_config(&config).expect_err("the spec is refused");

        assert!(error.to_string().contains("genesis_overrides"), "{error}");
    }

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

    /// The overrides only mean something to an external generator, so they pick it, with the
    /// node binary's own `gen-spec`.
    #[test]
    fn genesis_overrides_pick_the_node_binary_generator() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_default_command("/bin/node-binary")
            .with_validator(|node| node.with_name("jam0"))
            .with_genesis_overrides(serde_json::json!({}))
            .build()
            .expect("the chain config builds");

        let spec = JamchainSpec::from_config(&config).expect("the spec builds");

        assert_eq!(
            spec.chain_spec_source,
            JamChainSpecSource::Generator {
                command: "/bin/node-binary gen-spec".into()
            }
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
