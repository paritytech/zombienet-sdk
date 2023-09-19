use configuration::{shared::types::RegistrationStrategy, ParachainConfig};

use super::node::NodeSpec;
use crate::{
    errors::OrchestratorError,
    generators::{chain_spec::ChainSpec, para_artifact::*},
    shared::types::ChainDefaultContext,
};

#[derive(Debug)]
pub struct ParachainSpec {
    // `name` of the parachain (used in some corner cases)
    // name: Option<Chain>,
    pub(crate) id: u32,
    pub(crate) chain_spec: Option<ChainSpec>, // Only needed by cumulus based paras
    pub(crate) registration_strategy: RegistrationStrategy,
    pub(crate) onboard_as_parachain: bool,
    pub(crate) is_cumulus_based: bool,
    pub(crate) initial_balance: u128,
    pub(crate) genesis_state: ParaArtifact,
    pub(crate) genesis_wasm: ParaArtifact,
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

            if let Some(chain_spec_path) = config.chain_spec_path() {
                Some(ChainSpec::new(chain_name).asset_location(chain_spec_path.clone()).chain_name(chain_name))

            } else {
                // TODO: Do we need to add the posibility to set the command to use?
                // Currently (v1) is possible but when is set is set to the default command.
                Some(ChainSpec::new(chain_name).commad(main_cmd.as_str()).chain_name(chain_name))
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
