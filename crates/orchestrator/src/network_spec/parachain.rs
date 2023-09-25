use configuration::{
    shared::{resources::Resources, types::RegistrationStrategy},
    types::{Arg, AssetLocation, Command, Image},
    ParachainConfig,
};

use super::node::NodeSpec;
use crate::{
    errors::OrchestratorError,
    generators::{
        chain_spec::{ChainSpec, Context},
        para_artifact::*,
    },
    shared::types::ChainDefaultContext,
};

#[derive(Debug, Clone)]
pub struct ParachainSpec {
    // `name` of the parachain (used in some corner cases)
    // name: Option<Chain>,
    /// Parachain id
    pub(crate) id: u32,

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

    /// Chain-spec, only needed by cumulus based paras
    pub(crate) chain_spec: Option<ChainSpec>,

    /// Registration strategy to use
    pub(crate) registration_strategy: RegistrationStrategy,

    /// Oboard as parachain or parathread
    pub(crate) onboard_as_parachain: bool,

    /// Is the parachain cumulus-based
    pub(crate) is_cumulus_based: bool,

    /// Initial balance
    pub(crate) initial_balance: u128,

    /// Genesis state (head) to register the parachain
    pub(crate) genesis_state: ParaArtifact,

    /// Genesis wasm to register the parachain
    pub(crate) genesis_wasm: ParaArtifact,

    /// Collators to spawn
    pub(crate) collators: Vec<NodeSpec>,
}

impl ParachainSpec {
    pub fn from_config(config: &ParachainConfig) -> Result<ParachainSpec, OrchestratorError> {
        let main_cmd = if let Some(cmd) = config.default_command() {
            cmd
        } else if let Some(first_node) = config.collators().first() {
            let Some(cmd) = first_node.command() else {
                return Err(OrchestratorError::InvalidConfig("Parachain, either default_command or command in the first node needs to be set.".to_string()));
            };

            cmd
        } else {
            return Err(OrchestratorError::InvalidConfig(
                "Parachain without nodes and default_command isn't set.".to_string(),
            ));
        };

        let chain_spec = if config.is_cumulus_based() {
            // we need a chain-spec
            let chain_name = if let Some(chain_name) = config.chain() {
                chain_name.as_str()
            } else {
                ""
            };

            let chain_spec_builder = if chain_name.is_empty() {
                // if the chain don't have name use the id for the name of the file
                ChainSpec::new(config.id().to_string(), Context::Para)
            } else {
                ChainSpec::new(chain_name, Context::Para)
            };
            let chain_spec_builder = chain_spec_builder.set_chain_name(chain_name);

            if let Some(chain_spec_path) = config.chain_spec_path() {
                Some(chain_spec_builder.asset_location(chain_spec_path.clone()))
            } else {
                // TODO: Do we need to add the posibility to set the command to use?
                // Currently (v1) is possible but when is set is set to the default command.
                Some(chain_spec_builder.commad(main_cmd.as_str()))
            }
        } else {
            None
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
        let mut collators: Vec<NodeSpec> = Default::default();
        config.collators().iter().for_each(|node_config| {
            match NodeSpec::from_config(node_config, &chain_context) {
                Ok(node) => collators.push(node),
                Err(err) => errs.push(err),
            }
        });

        let genesis_state = if let Some(path) = config.genesis_state_path() {
            ParaArtifact::new(
                ParaArtifactType::State,
                ParaArtifactBuildOption::Path(path.to_string()),
            )
        } else {
            let cmd = if let Some(cmd) = config.genesis_state_generator() {
                cmd
            } else {
                main_cmd
            };
            ParaArtifact::new(
                ParaArtifactType::State,
                ParaArtifactBuildOption::Command(cmd.as_str().into()),
            )
        };

        let genesis_wasm = if let Some(path) = config.genesis_wasm_path() {
            ParaArtifact::new(
                ParaArtifactType::Wasm,
                ParaArtifactBuildOption::Path(path.to_string()),
            )
        } else {
            let cmd = if let Some(cmd) = config.genesis_wasm_generator() {
                cmd
            } else {
                main_cmd
            };
            ParaArtifact::new(
                ParaArtifactType::Wasm,
                ParaArtifactBuildOption::Command(cmd.as_str().into()),
            )
        };

        let para_spec = ParachainSpec {
            id: config.id(),
            default_command: config.default_command().cloned(),
            default_image: config.default_image().cloned(),
            default_resources: config.default_resources().cloned(),
            default_db_snapshot: config.default_db_snapshot().cloned(),
            default_args: config.default_args().into_iter().cloned().collect(),
            chain_spec,
            registration_strategy: config
                .registration_strategy()
                .unwrap_or(&RegistrationStrategy::InGenesis)
                .clone(),
            onboard_as_parachain: config.onboard_as_parachain(),
            is_cumulus_based: config.is_cumulus_based(),
            initial_balance: config.initial_balance(),
            genesis_state,
            genesis_wasm,
            collators,
        };

        Ok(para_spec)
    }
}
