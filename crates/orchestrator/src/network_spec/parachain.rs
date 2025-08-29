use std::{collections::HashMap, path::PathBuf};

use configuration::{
    shared::resources::Resources,
    types::{Arg, AssetLocation, Command, Image},
    ParachainConfig, RegistrationStrategy,
};
use provider::DynNamespace;
use serde::{Deserialize, Serialize};
use support::{fs::FileSystem, replacer::apply_replacements};
use tracing::debug;

use super::node::NodeSpec;
use crate::{
    errors::OrchestratorError,
    generators::{
        chain_spec::{ChainSpec, CommandInContext, Context, GenerationStrategy, ParaGenesisConfig},
        para_artifact::*,
    },
    shared::{
        constants::{
            DEFAULT_CHAIN_SPEC_TPL_COMMAND, DEFAULT_CHAIN_SPEC_TPL_USING_RUNTIME_DEFAULT_COMMAND,
            DEFAULT_CHAIN_SPEC_TPL_USING_RUNTIME_NAMED_PRESET_COMMAND,
            DEFAULT_LIST_PRESETS_TPL_COMMAND,
        },
        types::ChainDefaultContext,
    },
    ScopedFilesystem,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParachainSpec {
    // `name` of the parachain (used in some corner cases)
    // name: Option<Chain>,
    /// Parachain id
    pub(crate) id: u32,

    /// Unique id of the parachain, in the patter of <para_id>-<n>
    /// where the suffix is only present if more than one parachain is set with the same id
    pub(crate) unique_id: String,

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

    /// Chain-spec, only needed by cumulus based paras
    pub(crate) chain_spec: Option<ChainSpec>,

    /// Do not automatically assign a bootnode role if no nodes are marked as bootnodes.
    pub(crate) no_default_bootnodes: bool,

    /// Registration strategy to use
    pub(crate) registration_strategy: RegistrationStrategy,

    /// Onboard as parachain or parathread
    pub(crate) onboard_as_parachain: bool,

    /// Is the parachain cumulus-based
    pub(crate) is_cumulus_based: bool,

    /// Is the parachain evm-based
    pub(crate) is_evm_based: bool,

    /// Initial balance
    pub(crate) initial_balance: u128,

    /// Genesis state (head) to register the parachain
    pub(crate) genesis_state: ParaArtifact,

    /// Genesis WASM to register the parachain
    pub(crate) genesis_wasm: ParaArtifact,

    /// Genesis overrides as JSON value.
    pub(crate) genesis_overrides: Option<serde_json::Value>,

    /// Wasm override path/url to use.
    pub(crate) wasm_override: Option<AssetLocation>,

    /// Collators to spawn
    pub(crate) collators: Vec<NodeSpec>,
}

impl ParachainSpec {
    pub fn from_config(config: &ParachainConfig) -> Result<ParachainSpec, OrchestratorError> {
        let main_cmd = if let Some(cmd) = config.default_command() {
            cmd
        } else if let Some(first_node) = config.collators().first() {
            let Some(cmd) = first_node.command() else {
                return Err(OrchestratorError::InvalidConfig(format!("Parachain {}, either default_command or command in the first node needs to be set.", config.id())));
            };

            cmd
        } else {
            return Err(OrchestratorError::InvalidConfig(format!(
                "Parachain {}, without nodes and default_command isn't set.",
                config.id()
            )));
        };

        // TODO: internally we use image as String
        let main_image = config
            .default_image()
            .or(config.collators().first().and_then(|node| node.image()))
            .map(|image| image.as_str().to_string());

        let chain_spec = if config.is_cumulus_based() {
            // we need a chain-spec
            let chain_name = config.chain().map(|ch| ch.as_str()).unwrap_or("");

            let replacements = HashMap::from([
                ("disableBootnodes", "--disable-default-bootnode"),
                ("mainCommand", main_cmd.as_str()),
            ]);

            let tmpl = if let Some(tmpl) = config.chain_spec_command() {
                apply_replacements(tmpl, &replacements)
            } else {
                apply_replacements(DEFAULT_CHAIN_SPEC_TPL_COMMAND, &replacements)
            };

            let generation_strategy = {
                if let Some(chain_spec_path) = config.chain_spec_path() {
                    GenerationStrategy::WithAssetLocation {
                        asset_location: chain_spec_path.clone(),
                        build_raw_command: CommandInContext::new(
                            tmpl,
                            config.chain_spec_command_is_local(),
                        ),
                    }
                } else if config.chain_spec_command().is_some() {
                    GenerationStrategy::WithCommand(CommandInContext::new(
                        tmpl,
                        config.chain_spec_command_is_local(),
                    ))
                } else if main_cmd.as_str().ends_with("polkadot-parachain")
                    && config.runtime_path().is_some()
                {
                    let is_local = config.chain_spec_command_is_local();

                    let runtime_path = config.runtime_path().unwrap().clone();

                    let build_with_preset_command = apply_replacements(
                        DEFAULT_CHAIN_SPEC_TPL_USING_RUNTIME_NAMED_PRESET_COMMAND,
                        &replacements,
                    );
                    let build_with_preset_command =
                        CommandInContext::new(build_with_preset_command, is_local);

                    let build_default_command = apply_replacements(
                        DEFAULT_CHAIN_SPEC_TPL_USING_RUNTIME_DEFAULT_COMMAND,
                        &replacements,
                    );
                    let build_default_command =
                        CommandInContext::new(build_default_command, is_local);

                    let build_raw_command =
                        apply_replacements(DEFAULT_CHAIN_SPEC_TPL_COMMAND, &replacements);
                    let build_raw_command = CommandInContext::new(build_raw_command, is_local);

                    let list_presets_command =
                        apply_replacements(DEFAULT_LIST_PRESETS_TPL_COMMAND, &replacements);
                    let list_presets_command =
                        CommandInContext::new(list_presets_command, is_local);

                    GenerationStrategy::WithChainSpecBuilder {
                        build_with_preset_command,
                        build_default_command,
                        build_raw_command,
                        list_presets_command,
                        runtime_path,
                    }
                } else {
                    GenerationStrategy::WithCommand(CommandInContext::new(
                        tmpl,
                        config.chain_spec_command_is_local(),
                    ))
                }
            };

            let chain_spec_builder = if chain_name.is_empty() {
                // if the chain don't have name use the unique_id for the name of the file
                ChainSpec::new(
                    config.unique_id().to_string(),
                    Context::Para,
                    generation_strategy,
                )
            } else {
                let chain_spec_file_name = if config.unique_id().contains('-') {
                    &format!("{}-{}", chain_name, config.unique_id())
                } else {
                    chain_name
                };
                ChainSpec::new(chain_spec_file_name, Context::Para, generation_strategy)
            };
            let chain_spec_builder = chain_spec_builder.set_chain_name(chain_name);

            let chain_spec = chain_spec_builder.image(main_image.clone());
            Some(chain_spec)
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
            match NodeSpec::from_config(node_config, &chain_context, true) {
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
                cmd.cmd()
            } else {
                main_cmd
            };
            ParaArtifact::new(
                ParaArtifactType::State,
                ParaArtifactBuildOption::Command(cmd.as_str().into()),
            )
            .image(main_image.clone())
        };

        let genesis_wasm = if let Some(path) = config.genesis_wasm_path() {
            ParaArtifact::new(
                ParaArtifactType::Wasm,
                ParaArtifactBuildOption::Path(path.to_string()),
            )
        } else {
            let cmd = if let Some(cmd) = config.genesis_wasm_generator() {
                cmd.as_str()
            } else {
                main_cmd.as_str()
            };
            ParaArtifact::new(
                ParaArtifactType::Wasm,
                ParaArtifactBuildOption::Command(cmd.into()),
            )
            .image(main_image.clone())
        };

        let para_spec = ParachainSpec {
            id: config.id(),
            // ensure unique id is set at this point, if not just set to the para_id
            unique_id: if config.unique_id().is_empty() {
                config.id().to_string()
            } else {
                config.unique_id().to_string()
            },
            default_command: config.default_command().cloned(),
            default_image: config.default_image().cloned(),
            default_resources: config.default_resources().cloned(),
            default_db_snapshot: config.default_db_snapshot().cloned(),
            wasm_override: config.wasm_override().cloned(),
            default_args: config.default_args().into_iter().cloned().collect(),
            chain_spec,
            no_default_bootnodes: config.no_default_bootnodes(),
            registration_strategy: config
                .registration_strategy()
                .unwrap_or(&RegistrationStrategy::InGenesis)
                .clone(),
            onboard_as_parachain: config.onboard_as_parachain(),
            is_cumulus_based: config.is_cumulus_based(),
            is_evm_based: config.is_evm_based(),
            initial_balance: config.initial_balance(),
            genesis_state,
            genesis_wasm,
            genesis_overrides: config.genesis_overrides().cloned(),
            collators,
        };

        Ok(para_spec)
    }

    pub fn registration_strategy(&self) -> &RegistrationStrategy {
        &self.registration_strategy
    }

    pub fn get_genesis_config(&self) -> Result<ParaGenesisConfig<&PathBuf>, OrchestratorError> {
        let genesis_config = ParaGenesisConfig {
            state_path: self.genesis_state.artifact_path().ok_or(
                OrchestratorError::InvariantError(
                    "artifact path for state must be set at this point",
                ),
            )?,
            wasm_path: self.genesis_wasm.artifact_path().ok_or(
                OrchestratorError::InvariantError(
                    "artifact path for wasm must be set at this point",
                ),
            )?,
            id: self.id,
            as_parachain: self.onboard_as_parachain,
        };
        Ok(genesis_config)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn chain_spec(&self) -> Option<&ChainSpec> {
        self.chain_spec.as_ref()
    }

    pub fn chain_spec_mut(&mut self) -> Option<&mut ChainSpec> {
        self.chain_spec.as_mut()
    }

    /// Build parachain chain-spec
    ///
    /// This function customize the chain-spec (if is possible) and build the raw version
    /// of the chain-spec.
    pub(crate) async fn build_chain_spec<'a, T>(
        &mut self,
        relay_chain_id: &str,
        ns: &DynNamespace,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<Option<PathBuf>, anyhow::Error>
    where
        T: FileSystem,
    {
        let cloned = self.clone();
        let chain_spec_raw_path = if let Some(chain_spec) = self.chain_spec.as_mut() {
            debug!("parachain chain-spec building!");
            chain_spec.build(ns, scoped_fs).await?;
            debug!("parachain chain-spec built!");

            chain_spec
                .customize_para(&cloned, relay_chain_id, scoped_fs)
                .await?;
            debug!("parachain chain-spec customized!");
            chain_spec.build_raw(ns, scoped_fs).await?;
            debug!("parachain chain-spec raw built!");

            // override wasm if needed
            if let Some(ref wasm_override) = self.wasm_override {
                chain_spec.override_code(scoped_fs, wasm_override).await?;
            }

            let chain_spec_raw_path =
                chain_spec
                    .raw_path()
                    .ok_or(OrchestratorError::InvariantError(
                        "chain-spec raw path should be set now",
                    ))?;

            Some(chain_spec_raw_path.to_path_buf())
        } else {
            None
        };
        Ok(chain_spec_raw_path)
    }
}
