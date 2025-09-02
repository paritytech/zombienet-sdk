use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use configuration::{types::AssetLocation, HrmpChannelConfig};
use provider::{
    constants::NODE_CONFIG_DIR,
    types::{GenerateFileCommand, GenerateFilesOptions, TransferedFile},
    DynNamespace, ProviderError,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use support::{constants::THIS_IS_A_BUG, fs::FileSystem, replacer::apply_replacements};
use tokio::process::Command;
use tracing::{debug, info, trace, warn};

use super::errors::GeneratorError;
use crate::{
    network_spec::{node::NodeSpec, parachain::ParachainSpec, relaychain::RelaychainSpec},
    ScopedFilesystem,
};

#[derive(Debug, Deserialize, Default)]
struct ListPresetsResult {
    presets: Vec<String>,
}

// TODO: (javier) move to state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Context {
    Relay,
    Para,
}

enum ChainSpecFormat {
    Plain,
    Raw,
}

enum KeyType {
    Session,
    Aura,
    Grandpa,
}

#[derive(Debug, Clone, Copy)]
enum SessionKeyType {
    Default,
    Stash,
    Evm,
}

impl Default for SessionKeyType {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandInContext {
    Local(String),
    Remote(String),
}

impl CommandInContext {
    pub(crate) fn new(command: impl Into<String>, is_local: bool) -> Self {
        if is_local {
            CommandInContext::Local(command.into())
        } else {
            CommandInContext::Remote(command.into())
        }
    }

    fn cmd(&self) -> &str {
        match self {
            CommandInContext::Local(cmd) | CommandInContext::Remote(cmd) => cmd.as_ref(),
        }
    }
}

#[derive(Debug)]
pub struct ParaGenesisConfig<T: AsRef<Path>> {
    pub(crate) state_path: T,
    pub(crate) wasm_path: T,
    pub(crate) id: u32,
    pub(crate) as_parachain: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Strategy used for chainspec generation
pub enum GenerationStrategy {
    // Uses asset_location as chainspec source, build_raw command is used if provided chainspec is plain to build the raw version.
    WithAssetLocation {
        asset_location: AssetLocation,
        build_raw_command: CommandInContext,
    },
    // Uses the provided command to build the chainspec.
    WithCommand(CommandInContext),
    // Uses chain-spec-builder, first we list available presets for a given runtime, if there's a matching preset we use it
    // otherwise we use the default.
    WithChainSpecBuilder {
        build_with_preset_command: CommandInContext,
        build_default_command: CommandInContext,
        build_raw_command: CommandInContext,
        list_presets_command: CommandInContext,
        runtime_path: AssetLocation,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSpec {
    // Name of the spec file, most of the times could be the same as the chain_name. (e.g rococo-local)
    chain_spec_name: String,
    maybe_plain_path: Option<PathBuf>,
    chain_name: Option<String>,
    raw_path: Option<PathBuf>,
    // Strategy used for chain-spec generation.
    generation_strategy: GenerationStrategy,
    // Image to use for build the chain-spec
    image: Option<String>,
    // Contex of the network (e.g relay or para)
    context: Context,
}

impl ChainSpec {
    pub(crate) fn new(
        chain_spec_name: impl Into<String>,
        context: Context,
        generation_strategy: GenerationStrategy,
    ) -> Self {
        Self {
            chain_spec_name: chain_spec_name.into(),
            chain_name: None,
            maybe_plain_path: None,
            raw_path: None,
            image: None,
            context,
            generation_strategy,
        }
    }

    pub(crate) fn chain_spec_name(&self) -> &str {
        self.chain_spec_name.as_ref()
    }

    pub(crate) fn chain_name(&self) -> Option<&str> {
        self.chain_name.as_deref()
    }

    pub(crate) fn set_chain_name(mut self, chain_name: impl Into<String>) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    pub(crate) fn image(mut self, image: Option<String>) -> Self {
        self.image = image;
        self
    }

    /// Build the chain-spec
    pub async fn build<'a, T>(
        &mut self,
        ns: &DynNamespace,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        let maybe_plain_spec_path = PathBuf::from(format!("{}-plain.json", self.chain_spec_name));

        match &self.generation_strategy {
            // if we have a path, copy to the base_dir of the ns with the name `<name>-plain.json`
            GenerationStrategy::WithAssetLocation { asset_location, .. } => {
                trace!("building chainspec using GenerationStrategy::WithAssetLocation");

                copy_from_location(scoped_fs, asset_location, maybe_plain_spec_path.clone()).await?
            },
            GenerationStrategy::WithCommand(command) => {
                trace!("building chainspec using GenerationStrategy::WithCommand");

                // we should create the chain-spec using command.
                let mut replacement_value = String::default();
                if let Some(chain_name) = self.chain_name.as_ref() {
                    if !chain_name.is_empty() {
                        replacement_value.clone_from(chain_name);
                    }
                };

                let sanitized_cmd = if replacement_value.is_empty() {
                    // we need to remove the `--chain` flag
                    command.cmd().replace("--chain", "")
                } else {
                    command.cmd().to_owned()
                };

                let full_cmd = apply_replacements(
                    &sanitized_cmd,
                    &HashMap::from([("chainName", replacement_value.as_str())]),
                );
                trace!("full_cmd: {:?}", full_cmd);

                let parts: Vec<&str> = full_cmd.split_whitespace().collect();
                let Some((cmd, args)) = parts.split_first() else {
                    return Err(GeneratorError::ChainSpecGeneration(format!(
                        "Invalid generator command: {full_cmd}"
                    )));
                };
                trace!("cmd: {:?} - args: {:?}", cmd, args);

                let generate_command =
                    GenerateFileCommand::new(cmd, maybe_plain_spec_path.clone()).args(args);
                let options =
                    GenerateFilesOptions::new(vec![generate_command.clone()], self.image.clone());

                execute_command(ns, scoped_fs, command, generate_command, options).await?;
            },
            GenerationStrategy::WithChainSpecBuilder {
                build_with_preset_command,
                build_default_command,
                list_presets_command,
                runtime_path,
                ..
            } => {
                trace!("building chainspec using GenerationStrategy::WithChainSpecBuilder");

                let path = PathBuf::from(format!("runtime-{}.wasm", self.chain_spec_name));
                copy_from_location(scoped_fs, runtime_path, path.clone()).await?;

                let cmd_in_context = if self.chain_name.is_none() {
                    // if chain name is empty we use the default command
                    build_default_command
                } else {
                    // we list presets and if there's a matching preset we use it, else use the default command
                    let temp_name = format!(
                        "temp-runtime-{}-{}",
                        self.chain_spec_name,
                        rand::random::<u8>()
                    );

                    let (runtime_path_local, runtime_path_in_pod, runtime_path_in_args) =
                        self.build_paths(ns, &path, &temp_name);

                    let replacements =
                        HashMap::from([("runtimePath", runtime_path_in_args.as_str())]);
                    let list_presets_cmd =
                        apply_replacements(list_presets_command.cmd(), &replacements);

                    trace!("list_presets_cmd: {:?}", list_presets_cmd);

                    let parts: Vec<&str> = list_presets_cmd.split_whitespace().collect();
                    let Some((cmd, args)) = parts.split_first() else {
                        return Err(GeneratorError::ChainSpecGeneration(format!(
                            "Invalid generator command: {list_presets_cmd}"
                        )));
                    };
                    trace!("cmd: {:?} - args: {:?}", cmd, args);

                    let list_presets_local_output_path =
                        PathBuf::from(format!("list-presets-result-{}", self.chain_spec_name));

                    let generate_command =
                        GenerateFileCommand::new(cmd, list_presets_local_output_path.clone())
                            .args(args);

                    let options = GenerateFilesOptions::with_files(
                        vec![generate_command.clone()],
                        self.image.clone(),
                        &[TransferedFile::new(
                            runtime_path_local.clone(),
                            runtime_path_in_pod.clone(),
                        )],
                    )
                    .temp_name(&temp_name);

                    execute_command(
                        ns,
                        scoped_fs,
                        list_presets_command,
                        generate_command,
                        options,
                    )
                    .await?;

                    let list_presets_result = scoped_fs
                        .read_to_string(list_presets_local_output_path)
                        .await?;

                    let list_presets_result: ListPresetsResult =
                        serde_json::from_str(&list_presets_result).unwrap_or_default();

                    trace!("found presets: {list_presets_result:?}");

                    if list_presets_result
                        .presets
                        .contains(self.chain_name.as_ref().unwrap())
                    {
                        build_with_preset_command
                    } else {
                        build_default_command
                    }
                };

                let temp_name = format!(
                    "temp-runtime-{}-{}",
                    self.chain_spec_name,
                    rand::random::<u8>()
                );

                let (runtime_path_local, runtime_path_in_pod, runtime_path_in_args) =
                    self.build_paths(ns, &path, &temp_name);

                let mut replacement_value = String::default();

                if let Some(chain_name) = self.chain_name.as_ref() {
                    if !chain_name.is_empty() {
                        replacement_value.clone_from(chain_name);
                    }
                };

                let sanitized_cmd = if replacement_value.is_empty() {
                    cmd_in_context.cmd().replace("--chain-name", "")
                } else {
                    cmd_in_context.cmd().to_owned()
                };

                // as opposed to build-spec, chain-spec-builder doesn't write the result to stdout automatically,
                // but we can redirect the output to stdout
                let output_path = "/dev/stdout";

                let replacements = HashMap::from([
                    ("runtimePath", runtime_path_in_args.as_str()),
                    ("chainName", self.chain_name.as_deref().unwrap()),
                    ("outputPath", output_path),
                ]);

                let full_cmd = apply_replacements(&sanitized_cmd, &replacements);

                trace!("full_cmd: {:?}", full_cmd);

                let parts: Vec<&str> = full_cmd.split_whitespace().collect();
                let Some((cmd, args)) = parts.split_first() else {
                    return Err(GeneratorError::ChainSpecGeneration(format!(
                        "Invalid generator command: {full_cmd}"
                    )));
                };

                trace!("cmd: {:?} - args: {:?}", cmd, args);

                let generate_command =
                    GenerateFileCommand::new(cmd, maybe_plain_spec_path.clone()).args(args);
                let options = GenerateFilesOptions::with_files(
                    vec![generate_command.clone()],
                    self.image.clone(),
                    &[TransferedFile::new(
                        runtime_path_local.clone(),
                        runtime_path_in_pod.clone(),
                    )],
                )
                .temp_name(temp_name);

                execute_command(ns, scoped_fs, cmd_in_context, generate_command, options).await?;
            },
        };

        if is_raw(maybe_plain_spec_path.clone(), scoped_fs).await? {
            let spec_path = PathBuf::from(format!("{}.json", self.chain_spec_name));
            let tf_file = TransferedFile::new(
                &PathBuf::from_iter([ns.base_dir(), &maybe_plain_spec_path]),
                &spec_path,
            );
            scoped_fs.copy_files(vec![&tf_file]).await.map_err(|e| {
                GeneratorError::ChainSpecGeneration(format!(
                    "Error copying file: {tf_file}, err: {e}"
                ))
            })?;

            self.raw_path = Some(spec_path);
        } else {
            self.maybe_plain_path = Some(maybe_plain_spec_path);
        }
        Ok(())
    }

    pub async fn build_raw<'a, T>(
        &mut self,
        ns: &DynNamespace,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        let None = self.raw_path else {
            return Ok(());
        };
        // build raw
        let temp_name = format!(
            "temp-build-raw-{}-{}",
            self.chain_spec_name,
            rand::random::<u8>()
        );
        let raw_spec_path = PathBuf::from(format!("{}.json", self.chain_spec_name));
        let cmd = self.build_raw_command();
        let maybe_plain_path =
            self.maybe_plain_path
                .as_ref()
                .ok_or(GeneratorError::ChainSpecGeneration(
                    "Invalid plain path".into(),
                ))?;

        let (chain_spec_path_local, chain_spec_path_in_pod, chain_spec_path_in_args) =
            self.build_paths(ns, maybe_plain_path, &temp_name);

        let mut full_cmd = apply_replacements(
            cmd.cmd(),
            &HashMap::from([("chainName", chain_spec_path_in_args.as_str())]),
        );

        if !full_cmd.contains("--raw") {
            full_cmd = format!("{full_cmd} --raw");
        }
        trace!("full_cmd: {:?}", full_cmd);

        let parts: Vec<&str> = full_cmd.split_whitespace().collect();
        let Some((cmd, args)) = parts.split_first() else {
            return Err(GeneratorError::ChainSpecGeneration(format!(
                "Invalid generator command: {full_cmd}"
            )));
        };
        trace!("cmd: {:?} - args: {:?}", cmd, args);

        let generate_command = GenerateFileCommand::new(cmd, raw_spec_path.clone()).args(args);
        let options = GenerateFilesOptions::with_files(
            vec![generate_command.clone()],
            self.image.clone(),
            &[TransferedFile::new(
                chain_spec_path_local,
                chain_spec_path_in_pod,
            )],
        )
        .temp_name(temp_name);

        execute_command(
            ns,
            scoped_fs,
            self.build_raw_command(),
            generate_command,
            options,
        )
        .await?;

        self.raw_path = Some(raw_spec_path);

        Ok(())
    }

    /// Override the :code in chain-spec raw version
    pub async fn override_code<'a, T>(
        &mut self,
        scoped_fs: &ScopedFilesystem<'a, T>,
        wasm_override: &AssetLocation,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        // first ensure we have the raw version of the chain-spec
        let Some(_) = self.raw_path else {
            return Err(GeneratorError::OverridingWasm(String::from(
                "Raw path should be set at this point.",
            )));
        };
        let (content, _) = self.read_spec(scoped_fs).await?;
        // read override wasm
        let override_content = wasm_override.get_asset().await.map_err(|_| {
            GeneratorError::OverridingWasm(format!(
                "Can not get asset to override wasm, asset: {wasm_override}"
            ))
        })?;

        // read spec  to json value
        let mut chain_spec_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| {
                GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into())
            })?;

        // override :code
        let Some(code) = chain_spec_json.pointer_mut("/genesis/raw/top/0x3a636f6465") else {
            return Err(GeneratorError::OverridingWasm(String::from(
                "Pointer '/genesis/raw/top/0x3a636f6465' should be valid in the raw spec.",
            )));
        };

        info!(
            "🖋  Overriding ':code' (0x3a636f6465) in raw chain-spec with content of {}",
            wasm_override
        );
        *code = json!(format!("0x{}", hex::encode(override_content)));

        let overrided_content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
            GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
        })?;
        // save it
        self.write_spec(scoped_fs, overrided_content).await?;

        Ok(())
    }

    pub fn raw_path(&self) -> Option<&Path> {
        self.raw_path.as_deref()
    }

    pub async fn read_chain_id<'a, T>(
        &self,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<String, GeneratorError>
    where
        T: FileSystem,
    {
        let (content, _) = self.read_spec(scoped_fs).await?;
        ChainSpec::chain_id_from_spec(&content)
    }

    async fn read_spec<'a, T>(
        &self,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(String, ChainSpecFormat), GeneratorError>
    where
        T: FileSystem,
    {
        let (path, format) = match (self.maybe_plain_path.as_ref(), self.raw_path.as_ref()) {
            (Some(path), None) => (path, ChainSpecFormat::Plain),
            (None, Some(path)) => (path, ChainSpecFormat::Raw),
            (Some(_), Some(path)) => {
                // if we have both paths return the raw
                (path, ChainSpecFormat::Raw)
            },
            (None, None) => unreachable!(),
        };

        let content = scoped_fs.read_to_string(path.clone()).await.map_err(|_| {
            GeneratorError::ChainSpecGeneration(format!(
                "Can not read chain-spec from {}",
                path.to_string_lossy()
            ))
        })?;

        Ok((content, format))
    }

    async fn write_spec<'a, T>(
        &self,
        scoped_fs: &ScopedFilesystem<'a, T>,
        content: impl Into<String>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        let (path, _format) = match (self.maybe_plain_path.as_ref(), self.raw_path.as_ref()) {
            (Some(path), None) => (path, ChainSpecFormat::Plain),
            (None, Some(path)) => (path, ChainSpecFormat::Raw),
            (Some(_), Some(path)) => {
                // if we have both paths return the raw
                (path, ChainSpecFormat::Raw)
            },
            (None, None) => unreachable!(),
        };

        scoped_fs.write(path, content.into()).await.map_err(|_| {
            GeneratorError::ChainSpecGeneration(format!(
                "Can not write chain-spec from {}",
                path.to_string_lossy()
            ))
        })?;

        Ok(())
    }

    // TODO: (javier) move this fns to state aware
    pub async fn customize_para<'a, T>(
        &self,
        para: &ParachainSpec,
        relay_chain_id: &str,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        let (content, format) = self.read_spec(scoped_fs).await?;
        let mut chain_spec_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| {
                GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into())
            })?;

        if let Some(para_id) = chain_spec_json.get_mut("para_id") {
            *para_id = json!(para.id);
        };
        if let Some(para_id) = chain_spec_json.get_mut("paraId") {
            *para_id = json!(para.id);
        };

        if let Some(relay_chain_id_field) = chain_spec_json.get_mut("relay_chain") {
            *relay_chain_id_field = json!(relay_chain_id);
        };

        if let ChainSpecFormat::Plain = format {
            let pointer = get_runtime_config_pointer(&chain_spec_json)
                .map_err(GeneratorError::ChainSpecGeneration)?;

            // make genesis overrides first.
            if let Some(overrides) = &para.genesis_overrides {
                let percolated_overrides = percolate_overrides(&pointer, overrides)
                    .map_err(|e| GeneratorError::ChainSpecGeneration(e.to_string()))?;
                if let Some(genesis) = chain_spec_json.pointer_mut(&pointer) {
                    merge(genesis, percolated_overrides);
                }
            }

            clear_authorities(&pointer, &mut chain_spec_json);

            let key_type_to_use = if para.is_evm_based {
                SessionKeyType::Evm
            } else {
                SessionKeyType::Default
            };

            // Get validators to add as authorities
            let validators: Vec<&NodeSpec> = para
                .collators
                .iter()
                .filter(|node| node.is_validator)
                .collect();

            // check chain key types
            if chain_spec_json
                .pointer(&format!("{pointer}/session"))
                .is_some()
            {
                add_authorities(&pointer, &mut chain_spec_json, &validators, key_type_to_use);
            } else if chain_spec_json
                .pointer(&format!("{pointer}/aura"))
                .is_some()
            {
                add_aura_authorities(&pointer, &mut chain_spec_json, &validators, KeyType::Aura);
            } else {
                warn!("Can't customize keys, not `session` or `aura` find in the chain-spec file");
            };

            // Add nodes to collator
            let invulnerables: Vec<&NodeSpec> = para
                .collators
                .iter()
                .filter(|node| node.is_invulnerable)
                .collect();

            add_collator_selection(
                &pointer,
                &mut chain_spec_json,
                &invulnerables,
                key_type_to_use,
            );

            // override `parachainInfo/parachainId`
            override_parachain_info(&pointer, &mut chain_spec_json, para.id);

            // write spec
            let content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
                GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
            })?;
            self.write_spec(scoped_fs, content).await?;
        } else {
            warn!("⚠️ Chain spec for para_id: {} is in raw mode", para.id);
        }
        Ok(())
    }

    pub async fn customize_relay<'a, T, U>(
        &self,
        relaychain: &RelaychainSpec,
        hrmp_channels: &[HrmpChannelConfig],
        para_artifacts: Vec<ParaGenesisConfig<U>>,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
        U: AsRef<Path>,
    {
        let (content, format) = self.read_spec(scoped_fs).await?;
        let mut chain_spec_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| {
                GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into())
            })?;

        if let ChainSpecFormat::Plain = format {
            // get the tokenDecimals property or set the default (12)
            let token_decimals =
                if let Some(val) = chain_spec_json.pointer("/properties/tokenDecimals") {
                    let val = val.as_u64().unwrap_or(12);
                    if val > u8::MAX as u64 {
                        12
                    } else {
                        val as u8
                    }
                } else {
                    12
                };
            // get the config pointer
            let pointer = get_runtime_config_pointer(&chain_spec_json)
                .map_err(GeneratorError::ChainSpecGeneration)?;

            // make genesis overrides first.
            if let Some(overrides) = &relaychain.runtime_genesis_patch {
                let percolated_overrides = percolate_overrides(&pointer, overrides)
                    .map_err(|e| GeneratorError::ChainSpecGeneration(e.to_string()))?;
                if let Some(patch_section) = chain_spec_json.pointer_mut(&pointer) {
                    merge(patch_section, percolated_overrides);
                }
            }

            // get min stake (to store if neede later)
            let staking_min = get_staking_min(&pointer, &mut chain_spec_json);

            // Clear authorities
            clear_authorities(&pointer, &mut chain_spec_json);

            // add balances
            add_balances(
                &pointer,
                &mut chain_spec_json,
                &relaychain.nodes,
                token_decimals,
                staking_min,
            );

            // add staking
            add_staking(
                &pointer,
                &mut chain_spec_json,
                &relaychain.nodes,
                staking_min,
            );

            // Get validators to add as authorities
            let validators: Vec<&NodeSpec> = relaychain
                .nodes
                .iter()
                .filter(|node| node.is_validator)
                .collect();

            // check chain key types
            if chain_spec_json
                .pointer(&format!("{pointer}/session"))
                .is_some()
            {
                add_authorities(
                    &pointer,
                    &mut chain_spec_json,
                    &validators,
                    SessionKeyType::Stash,
                );
            } else {
                add_aura_authorities(&pointer, &mut chain_spec_json, &validators, KeyType::Aura);
                add_grandpa_authorities(&pointer, &mut chain_spec_json, &validators, KeyType::Aura);
            }

            // staking && nominators

            if !hrmp_channels.is_empty() {
                add_hrmp_channels(&pointer, &mut chain_spec_json, hrmp_channels);
            }

            // paras
            for para_genesis_config in para_artifacts.iter() {
                add_parachain_to_genesis(
                    &pointer,
                    &mut chain_spec_json,
                    para_genesis_config,
                    scoped_fs,
                )
                .await
                .map_err(|e| GeneratorError::ChainSpecGeneration(e.to_string()))?;
            }

            // TODO:
            // - staking
            // - nominators

            // write spec
            let content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
                GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
            })?;
            self.write_spec(scoped_fs, content).await?;
        } else {
            warn!(
                "⚠️ Chain Spec for chain {} is in raw mode, can't customize.",
                self.chain_spec_name
            );
        }
        Ok(())
    }

    pub async fn add_bootnodes<'a, T>(
        &self,
        scoped_fs: &ScopedFilesystem<'a, T>,
        bootnodes: &[String],
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        let (content, _) = self.read_spec(scoped_fs).await?;
        let mut chain_spec_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| {
                GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into())
            })?;

        if let Some(bootnodes_on_file) = chain_spec_json.get_mut("bootNodes") {
            if let Some(bootnodes_on_file) = bootnodes_on_file.as_array_mut() {
                let mut bootnodes_to_add =
                    bootnodes.iter().map(|bootnode| json!(bootnode)).collect();
                bootnodes_on_file.append(&mut bootnodes_to_add);
            } else {
                return Err(GeneratorError::ChainSpecGeneration(
                    "id should be an string in the chain-spec, this is a bug".into(),
                ));
            };
        } else {
            return Err(GeneratorError::ChainSpecGeneration(
                "'bootNodes' should be a fields in the chain-spec of the relaychain".into(),
            ));
        };

        // write spec
        let content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
            GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
        })?;
        self.write_spec(scoped_fs, content).await?;

        Ok(())
    }

    /// Get the chain_is from the json content of a chain-spec file.
    pub fn chain_id_from_spec(spec_content: &str) -> Result<String, GeneratorError> {
        let chain_spec_json: serde_json::Value =
            serde_json::from_str(spec_content).map_err(|_| {
                GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into())
            })?;
        if let Some(chain_id) = chain_spec_json.get("id") {
            if let Some(chain_id) = chain_id.as_str() {
                Ok(chain_id.to_string())
            } else {
                Err(GeneratorError::ChainSpecGeneration(
                    "id should be an string in the chain-spec, this is a bug".into(),
                ))
            }
        } else {
            Err(GeneratorError::ChainSpecGeneration(
                "'id' should be a fields in the chain-spec of the relaychain".into(),
            ))
        }
    }

    fn build_paths(
        &self,
        ns: &DynNamespace,
        path: &Path,
        temp_name: &str,
    ) -> (String, String, String) {
        // TODO: we should get the full path from the scoped filesystem
        let local_path = format!("{}/{}", ns.base_dir().to_string_lossy(), path.display());

        // Remote path to be injected
        let path_in_pod = format!("{}/{}", NODE_CONFIG_DIR, path.display());

        // Path in the context of the node, this can be different in the context of the providers (e.g native)
        let path_in_args = if matches!(self.build_raw_command(), CommandInContext::Local(_)) {
            local_path.clone()
        } else if ns.capabilities().prefix_with_full_path {
            // In native
            format!(
                "{}/{}{}",
                ns.base_dir().to_string_lossy(),
                &temp_name,
                &path_in_pod
            )
        } else {
            path_in_pod.clone()
        };

        (local_path, path_in_pod, path_in_args)
    }

    fn build_raw_command(&self) -> &CommandInContext {
        match &self.generation_strategy {
            GenerationStrategy::WithAssetLocation {
                build_raw_command, ..
            } => build_raw_command,
            GenerationStrategy::WithCommand(build_raw_command) => build_raw_command,
            GenerationStrategy::WithChainSpecBuilder {
                build_raw_command, ..
            } => build_raw_command,
        }
    }
}

type GenesisNodeKey = (String, String, HashMap<String, String>);

async fn build_locally<'a, T>(
    generate_command: GenerateFileCommand,
    scoped_fs: &ScopedFilesystem<'a, T>,
) -> Result<(), GeneratorError>
where
    T: FileSystem,
{
    // generate_command.

    let result = Command::new(generate_command.program.clone())
        .args(generate_command.args.clone())
        .current_dir(scoped_fs.base_dir)
        .output()
        .await
        .map_err(|err| {
            GeneratorError::ChainSpecGeneration(format!(
                "Error running cmd: {} args: {}, err: {}",
                &generate_command.program,
                &generate_command.args.join(" "),
                err
            ))
        })?;

    if result.status.success() {
        scoped_fs
            .write(
                generate_command.local_output_path,
                String::from_utf8_lossy(&result.stdout).to_string(),
            )
            .await?;
        Ok(())
    } else {
        Err(GeneratorError::ChainSpecGeneration(format!(
            "Error running cmd: {} args: {}, err: {}",
            &generate_command.program,
            &generate_command.args.join(" "),
            String::from_utf8_lossy(&result.stderr)
        )))
    }
}

async fn is_raw<'a, T>(
    file: PathBuf,
    scoped_fs: &ScopedFilesystem<'a, T>,
) -> Result<bool, ProviderError>
where
    T: FileSystem,
{
    let content = scoped_fs.read_to_string(file).await?;
    let chain_spec_json: serde_json::Value = serde_json::from_str(&content).unwrap();

    Ok(chain_spec_json.pointer("/genesis/raw/top").is_some())
}

async fn copy_from_location<'a, T>(
    scoped_fs: &ScopedFilesystem<'a, T>,
    location: &AssetLocation,
    destination: PathBuf,
) -> Result<(), GeneratorError>
where
    T: FileSystem,
{
    match location {
        AssetLocation::FilePath(path) => {
            let file_to_transfer = TransferedFile::new(path.clone(), destination);

            scoped_fs
                .copy_files(vec![&file_to_transfer])
                .await
                .map_err(|_| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "Error copying file: {file_to_transfer}"
                    ))
                })?;
            Ok(())
        },
        AssetLocation::Url(url) => {
            let res = reqwest::get(url.as_str())
                .await
                .map_err(|err| ProviderError::DownloadFile(url.to_string(), err.into()))?;

            let contents: &[u8] = &res.bytes().await.unwrap();
            trace!("writing content from {} to: {destination:?}", url.as_str());
            scoped_fs.write(&destination, contents).await?;
            Ok(())
        },
    }
}

async fn execute_command<'a, T>(
    ns: &DynNamespace,
    scoped_fs: &ScopedFilesystem<'a, T>,
    command_in_context: &CommandInContext,
    generate_command: GenerateFileCommand,
    options: GenerateFilesOptions,
) -> Result<(), GeneratorError>
where
    T: FileSystem,
{
    if let CommandInContext::Local(_) = command_in_context {
        build_locally(generate_command, scoped_fs).await?;
    } else {
        ns.generate_files(options).await?;
    }
    Ok(())
}

// Internal Chain-spec customizations

async fn add_parachain_to_genesis<'a, T, U>(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    para_genesis_config: &ParaGenesisConfig<U>,
    scoped_fs: &ScopedFilesystem<'a, T>,
) -> Result<(), anyhow::Error>
where
    T: FileSystem,
    U: AsRef<Path>,
{
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        let paras_pointer = if val.get("paras").is_some() {
            "/paras/paras"
        } else if val.get("parachainsParas").is_some() {
            // For retro-compatibility with substrate pre Polkadot 0.9.5
            "/parachainsParas/paras"
        } else {
            // The config may not contain paras. Since chainspec allows to contain the RuntimeGenesisConfig patch we can inject it.
            val["paras"] = json!({ "paras": [] });
            "/paras/paras"
        };

        let paras = val.pointer_mut(paras_pointer).ok_or(anyhow!(
            "paras pointer should be valid {:?} ",
            paras_pointer
        ))?;
        let paras_vec = paras
            .as_array_mut()
            .ok_or(anyhow!("paras should be an array"))?;

        let head = scoped_fs
            .read_to_string(para_genesis_config.state_path.as_ref())
            .await?;
        let wasm = scoped_fs
            .read_to_string(para_genesis_config.wasm_path.as_ref())
            .await?;

        paras_vec.push(json!([
            para_genesis_config.id,
            [head.trim(), wasm.trim(), para_genesis_config.as_parachain]
        ]));

        Ok(())
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

fn get_runtime_config_pointer(chain_spec_json: &serde_json::Value) -> Result<String, String> {
    // runtime_genesis_config is no longer in ChainSpec after rococo runtime rework (refer to: https://github.com/paritytech/polkadot-sdk/pull/1256)
    // ChainSpec may contain a RuntimeGenesisConfigPatch
    let pointers = [
        "/genesis/runtimeGenesis/config",
        "/genesis/runtimeGenesis/patch",
        "/genesis/runtimeGenesisConfigPatch",
        "/genesis/runtime/runtime_genesis_config",
        "/genesis/runtime",
    ];

    for pointer in pointers {
        if chain_spec_json.pointer(pointer).is_some() {
            return Ok(pointer.to_string());
        }
    }

    Err("Can not find the runtime pointer".into())
}

fn percolate_overrides<'a>(
    pointer: &str,
    overrides: &'a serde_json::Value,
) -> Result<&'a serde_json::Value, anyhow::Error> {
    let pointer_parts = pointer.split('/').collect::<Vec<&str>>();
    trace!("pointer_parts: {pointer_parts:?}");

    let top_level = overrides
        .as_object()
        .ok_or_else(|| anyhow!("Overrides must be an object"))?;
    let top_level_key = top_level
        .keys()
        .next()
        .ok_or_else(|| anyhow!("Invalid override value: {:?}", overrides))?;
    trace!("top_level_key: {top_level_key}");
    let index = pointer_parts.iter().position(|x| *x == top_level_key);
    let Some(i) = index else {
        warn!("Top level key '{top_level_key}' isn't part of the pointer ({pointer}), returning without percolating");
        return Ok(overrides);
    };

    let p = if i == pointer_parts.len() - 1 {
        // top level key is at end of the pointer
        let p = format!("/{}", pointer_parts[i]);
        trace!("overrides pointer {p}");
        p
    } else {
        // example: pointer is `/genesis/runtimeGenesis/patch` and the overrides start at  `runtimeGenesis`
        let p = format!("/{}", pointer_parts[i..].join("/"));
        trace!("overrides pointer {p}");
        p
    };
    let overrides_to_use = overrides
        .pointer(&p)
        .ok_or_else(|| anyhow!("Invalid override value: {:?}", overrides))?;
    Ok(overrides_to_use)
}

#[allow(dead_code)]
fn construct_runtime_pointer_from_overrides(
    overrides: &serde_json::Value,
) -> Result<String, anyhow::Error> {
    if overrides.get("genesis").is_some() {
        // overrides already start with /genesis
        return Ok("/genesis".into());
    } else {
        // check if we are one level inner
        if let Some(top_level) = overrides.as_object() {
            let k = top_level
                .keys()
                .next()
                .ok_or_else(|| anyhow!("Invalid override value: {:?}", overrides))?;
            match k.as_str() {
                "runtimeGenesisConfigPatch" | "runtime" | "runtimeGenesis" => {
                    return Ok(("/genesis").into())
                },
                "config" | "path" => {
                    return Ok(("/genesis/runtimeGenesis").into());
                },
                "runtime_genesis_config" => {
                    return Ok(("/genesis/runtime").into());
                },
                _ => {},
            }
        }
    }

    Err(anyhow!("Can not find the runtime pointer"))
}

// Merge `patch_section` with `overrides`.
fn merge(patch_section: &mut serde_json::Value, overrides: &serde_json::Value) {
    trace!("patch: {:?}", patch_section);
    trace!("overrides: {:?}", overrides);
    if let (Some(genesis_obj), Some(overrides_obj)) =
        (patch_section.as_object_mut(), overrides.as_object())
    {
        for overrides_key in overrides_obj.keys() {
            trace!("overrides_key: {:?}", overrides_key);
            // we only want to override keys present in the genesis object
            if let Some(genesis_value) = genesis_obj.get_mut(overrides_key) {
                match (&genesis_value, overrides_obj.get(overrides_key)) {
                    // recurse if genesis value is an object
                    (serde_json::Value::Object(_), Some(overrides_value))
                        if overrides_value.is_object() =>
                    {
                        merge(genesis_value, overrides_value);
                    },
                    // override if genesis value not an object
                    (_, Some(overrides_value)) => {
                        trace!("overriding: {:?} / {:?}", genesis_value, overrides_value);
                        *genesis_value = overrides_value.clone();
                    },
                    _ => {
                        trace!("not match!");
                    },
                }
            } else {
                // Allow to add keys, see (https://github.com/paritytech/zombienet/issues/1614)
                warn!(
                    "key: {overrides_key} not present in genesis_obj: {:?} (adding key)",
                    genesis_obj
                );
                let overrides_value = overrides_obj.get(overrides_key).expect(&format!(
                    "overrides_key {overrides_key} should be present in the overrides obj. qed"
                ));
                genesis_obj.insert(overrides_key.clone(), overrides_value.clone());
            }
        }
    }
}

fn clear_authorities(runtime_config_ptr: &str, chain_spec_json: &mut serde_json::Value) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        // clear keys (session, aura, grandpa)
        if val.get("session").is_some() {
            val["session"]["keys"] = json!([]);
        }

        if val.get("aura").is_some() {
            val["aura"]["authorities"] = json!([]);
        }

        if val.get("grandpa").is_some() {
            val["grandpa"]["authorities"] = json!([]);
        }

        // clear collatorSelector
        if val.get("collatorSelection").is_some() {
            val["collatorSelection"]["invulnerables"] = json!([]);
        }

        // clear staking but not `validatorCount` if `devStakers` is set
        if val.get("staking").is_some() {
            val["staking"]["invulnerables"] = json!([]);
            val["staking"]["stakers"] = json!([]);

            if val["staking"]["devStakers"] == json!(null) {
                val["staking"]["validatorCount"] = json!(0);
            }
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

fn get_staking_min(runtime_config_ptr: &str, chain_spec_json: &mut serde_json::Value) -> u128 {
    // get min staking
    let staking_ptr = format!("{runtime_config_ptr}/staking/stakers");
    if let Some(stakers) = chain_spec_json.pointer(&staking_ptr) {
        // stakers should be an array
        let min = stakers[0][2].clone();
        min.as_u64().unwrap_or(0).into()
    } else {
        0
    }
}

fn add_balances(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &Vec<NodeSpec>,
    token_decimals: u8,
    staking_min: u128,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        let Some(balances) = val.pointer("/balances/balances") else {
            // should be a info log
            warn!("NO 'balances' key in runtime config, skipping...");
            return;
        };

        // create a balance map
        let mut balances_map = generate_balance_map(balances);
        for node in nodes {
            if node.initial_balance.eq(&0) {
                continue;
            };

            // TODO: handle error here and check the `accounts.accounts` design
            // Double down the minimal stake defined
            let balance = std::cmp::max(node.initial_balance, staking_min * 2);
            for k in ["sr", "sr_stash"] {
                let account = node.accounts.accounts.get(k).unwrap();
                balances_map.insert(account.address.clone(), balance);
            }
        }

        // ensure zombie account (//Zombie) have funds
        // we will use for internal usage (e.g new validators)
        balances_map.insert(
            "5FTcLfwFc7ctvqp3RhbEig6UuHLHcHVRujuUm8r21wy4dAR8".to_string(),
            1000 * 10_u128.pow(token_decimals as u32),
        );

        // convert the map and store again
        let new_balances: Vec<(&String, &u128)> =
            balances_map.iter().collect::<Vec<(&String, &u128)>>();

        val["balances"]["balances"] = json!(new_balances);
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

fn get_node_keys(
    node: &NodeSpec,
    session_key: SessionKeyType,
    asset_hub_polkadot: bool,
) -> GenesisNodeKey {
    let sr_account = node.accounts.accounts.get("sr").unwrap();
    let sr_stash = node.accounts.accounts.get("sr_stash").unwrap();
    let ed_account = node.accounts.accounts.get("ed").unwrap();
    let ec_account = node.accounts.accounts.get("ec").unwrap();
    let eth_account = node.accounts.accounts.get("eth").unwrap();
    let mut keys = HashMap::new();
    for k in [
        "babe",
        "im_online",
        "parachain_validator",
        "authority_discovery",
        "para_validator",
        "para_assignment",
        "aura",
        "nimbus",
        "vrf",
    ] {
        if k == "aura" && asset_hub_polkadot {
            keys.insert(k.to_string(), ed_account.address.clone());
            continue;
        }
        keys.insert(k.to_string(), sr_account.address.clone());
    }

    keys.insert("grandpa".to_string(), ed_account.address.clone());
    keys.insert("beefy".to_string(), ec_account.address.clone());
    keys.insert("eth".to_string(), eth_account.public_key.clone());

    let account_to_use = match session_key {
        SessionKeyType::Default => sr_account.address.clone(),
        SessionKeyType::Stash => sr_stash.address.clone(),
        SessionKeyType::Evm => format!("0x{}", eth_account.public_key),
    };

    (account_to_use.clone(), account_to_use, keys)
}
fn add_authorities(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &[&NodeSpec],
    session_key: SessionKeyType,
) {
    let asset_hub_polkadot = chain_spec_json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|id| id.starts_with("asset-hub-polkadot"))
        .unwrap_or_default();
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        if let Some(session_keys) = val.pointer_mut("/session/keys") {
            let keys: Vec<GenesisNodeKey> = nodes
                .iter()
                .map(|node| get_node_keys(node, session_key, asset_hub_polkadot))
                .collect();
            *session_keys = json!(keys);
        } else {
            warn!("⚠️  'session/keys' key not present in runtime config.");
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}
fn add_hrmp_channels(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    hrmp_channels: &[HrmpChannelConfig],
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        if let Some(preopen_hrmp_channels) = val.pointer_mut("/hrmp/preopenHrmpChannels") {
            let hrmp_channels = hrmp_channels
                .iter()
                .map(|c| {
                    (
                        c.sender(),
                        c.recipient(),
                        c.max_capacity(),
                        c.max_message_size(),
                    )
                })
                .collect::<Vec<_>>();
            *preopen_hrmp_channels = json!(hrmp_channels);
        } else {
            warn!("⚠️  'hrmp/preopenHrmpChannels' key not present in runtime config.");
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

fn add_aura_authorities(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &[&NodeSpec],
    _key_type: KeyType,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        if let Some(aura_authorities) = val.pointer_mut("/aura/authorities") {
            let keys: Vec<String> = nodes
                .iter()
                .map(|node| {
                    node.accounts
                        .accounts
                        .get("sr")
                        .expect(&format!(
                            "'sr' account should be set at spec computation {THIS_IS_A_BUG}"
                        ))
                        .address
                        .clone()
                })
                .collect();
            *aura_authorities = json!(keys);
        } else {
            warn!("⚠️  'aura/authorities' key not present in runtime config.");
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

fn add_grandpa_authorities(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &[&NodeSpec],
    _key_type: KeyType,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        if let Some(grandpa_authorities) = val.pointer_mut("/grandpa/authorities") {
            let keys: Vec<(String, usize)> = nodes
                .iter()
                .map(|node| {
                    (
                        node.accounts
                            .accounts
                            .get("ed")
                            .expect(&format!(
                                "'ed' account should be set at spec computation {THIS_IS_A_BUG}"
                            ))
                            .address
                            .clone(),
                        1,
                    )
                })
                .collect();
            *grandpa_authorities = json!(keys);
        } else {
            warn!("⚠️  'grandpa/authorities' key not present in runtime config.");
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

fn add_staking(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &Vec<NodeSpec>,
    staking_min: u128,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        let Some(_) = val.pointer("/staking") else {
            // should be a info log
            warn!("NO 'staking' key in runtime config, skipping...");
            return;
        };

        let mut stakers = vec![];
        let mut invulnerables = vec![];
        for node in nodes {
            let sr_stash_addr = &node
                .accounts
                .accounts
                .get("sr_stash")
                .expect("'sr_stash account should be defined for the node. qed")
                .address;
            stakers.push(json!([
                sr_stash_addr,
                sr_stash_addr,
                staking_min,
                "Validator"
            ]));

            if node.is_invulnerable {
                invulnerables.push(sr_stash_addr);
            }
        }

        val["staking"]["validatorCount"] = json!(stakers.len());
        val["staking"]["stakers"] = json!(stakers);
        val["staking"]["invulnerables"] = json!(invulnerables);
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

// TODO: (team)
// fn add_nominators() {}

// // TODO: (team) we should think a better way to use the decorators from
// // current version (ts).
// fn para_custom() { todo!() }
fn override_parachain_info(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    para_id: u32,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        if let Some(parachain_id) = val.pointer_mut("/parachainInfo/parachainId") {
            *parachain_id = json!(para_id)
        } else {
            // Add warning here!
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}
fn add_collator_selection(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &[&NodeSpec],
    session_key: SessionKeyType,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        let key_type = if let SessionKeyType::Evm = session_key {
            "eth"
        } else {
            "sr"
        };
        let keys: Vec<String> = nodes
            .iter()
            .map(|node| {
                node.accounts
                    .accounts
                    .get(key_type)
                    .expect(&format!(
                        "'sr' account should be set at spec computation {THIS_IS_A_BUG}"
                    ))
                    .address
                    .clone()
            })
            .collect();

        // collatorSelection.invulnerables
        if let Some(invulnerables) = val.pointer_mut("/collatorSelection/invulnerables") {
            *invulnerables = json!(keys);
        } else {
            // TODO: add a nice warning here.
            debug!("⚠️  'invulnerables' not present in spec, will not be customized");
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}

// Helpers
fn generate_balance_map(balances: &serde_json::Value) -> HashMap<String, u128> {
    // SAFETY: balances is always an array in chain-spec with items [k,v]
    let balances_map: HashMap<String, u128> =
        serde_json::from_value::<Vec<(String, u128)>>(balances.to_owned())
            .unwrap()
            .iter()
            .fold(HashMap::new(), |mut memo, balance| {
                memo.insert(balance.0.clone(), balance.1);
                memo
            });
    balances_map
}

#[cfg(test)]
mod tests {
    use std::fs;

    use configuration::HrmpChannelConfigBuilder;

    use super::*;
    use crate::{generators, shared::types::NodeAccounts};

    const ROCOCO_LOCAL_PLAIN_TESTING: &str = "./testing/rococo-local-plain.json";

    fn chain_spec_test(file: &str) -> serde_json::Value {
        let content = fs::read_to_string(file).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn chain_spec_with_stake() -> serde_json::Value {
        json!({"genesis": {
            "runtimeGenesis" : {
                "patch": {
                    "staking": {
                        "forceEra": "NotForcing",
                        "invulnerables": [
                          "5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY",
                          "5HpG9w8EBLe5XCrbczpwq5TSXvedjrBGCwqxK1iQ7qUsSWFc"
                        ],
                        "minimumValidatorCount": 1,
                        "slashRewardFraction": 100000000,
                        "stakers": [
                          [
                            "5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY",
                            "5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY",
                            100000000000001_u128,
                            "Validator"
                          ],
                          [
                            "5HpG9w8EBLe5XCrbczpwq5TSXvedjrBGCwqxK1iQ7qUsSWFc",
                            "5HpG9w8EBLe5XCrbczpwq5TSXvedjrBGCwqxK1iQ7qUsSWFc",
                            100000000000000_u128,
                            "Validator"
                          ]
                        ],
                        "validatorCount": 2
                    },
                }
            }
        }})
    }

    fn chain_spec_with_dev_stakers() -> serde_json::Value {
        json!({"genesis": {
            "runtimeGenesis" : {
                "patch": {
                    "staking": {
                        "activeEra": [
                            0,
                            0,
                            0
                        ],
                        "canceledPayout": 0,
                        "devStakers": [
                            2000,
                            25000
                        ],
                        "forceEra": "NotForcing",
                        "invulnerables": [],
                        "maxNominatorCount": null,
                        "maxValidatorCount": null,
                        "minNominatorBond": 0,
                        "minValidatorBond": 0,
                        "slashRewardFraction": 0,
                        "stakers": [],
                        "validatorCount": 500
                    },
                }
            }
        }})
    }

    #[test]
    fn get_min_stake_works() {
        let mut chain_spec_json = chain_spec_with_stake();

        let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();
        let min = get_staking_min(&pointer, &mut chain_spec_json);

        assert_eq!(100000000000001, min);
    }

    #[test]
    fn dev_stakers_not_override_count_works() {
        let mut chain_spec_json = chain_spec_with_dev_stakers();

        let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();
        clear_authorities(&pointer, &mut chain_spec_json);

        let validator_count = chain_spec_json
            .pointer(&format!("{pointer}/staking/validatorCount"))
            .unwrap();
        assert_eq!(validator_count, &json!(500));
    }

    #[test]
    fn dev_stakers_override_count_works() {
        let mut chain_spec_json = chain_spec_with_stake();

        let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();
        clear_authorities(&pointer, &mut chain_spec_json);

        let validator_count = chain_spec_json
            .pointer(&format!("{pointer}/staking/validatorCount"))
            .unwrap();
        assert_eq!(validator_count, &json!(0));
    }

    #[test]
    fn overrides_from_toml_works() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize)]
        struct MockConfig {
            #[serde(rename = "genesis", skip_serializing_if = "Option::is_none")]
            genesis_overrides: Option<serde_json::Value>,
        }

        let mut chain_spec_json = chain_spec_test(ROCOCO_LOCAL_PLAIN_TESTING);
        // Could also be  something like [genesis.runtimeGenesis.patch.balances]
        const TOML: &str = "[genesis.runtime.balances]
            devAccounts = [
            20000,
            1000000000000000000,
            \"//Sender//{}\"
        ]";
        let override_toml: MockConfig = toml::from_str(TOML).unwrap();
        let overrides = override_toml.genesis_overrides.unwrap();
        let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();

        let percolated_overrides = percolate_overrides(&pointer, &overrides)
            .map_err(|e| GeneratorError::ChainSpecGeneration(e.to_string()))
            .unwrap();
        trace!("percolated_overrides: {:#?}", percolated_overrides);
        if let Some(genesis) = chain_spec_json.pointer_mut(&pointer) {
            merge(genesis, percolated_overrides);
        }

        trace!("chain spec: {chain_spec_json:#?}");
        assert!(chain_spec_json
            .pointer("/genesis/runtime/balances/devAccounts")
            .is_some());
    }

    #[test]
    fn add_balances_works() {
        let mut spec_plain = chain_spec_test(ROCOCO_LOCAL_PLAIN_TESTING);
        let mut name = String::from("luca");
        let initial_balance = 1_000_000_000_000_u128;
        let seed = format!("//{}{name}", name.remove(0).to_uppercase());
        let accounts = NodeAccounts {
            accounts: generators::generate_node_keys(&seed).unwrap(),
            seed,
        };
        let node = NodeSpec {
            name,
            accounts,
            initial_balance,
            ..Default::default()
        };

        let nodes = vec![node];
        add_balances("/genesis/runtime", &mut spec_plain, &nodes, 12, 0);

        let new_balances = spec_plain
            .pointer("/genesis/runtime/balances/balances")
            .unwrap();

        let balances_map = generate_balance_map(new_balances);

        // sr and sr_stash keys exists
        let sr = nodes[0].accounts.accounts.get("sr").unwrap();
        let sr_stash = nodes[0].accounts.accounts.get("sr_stash").unwrap();
        assert_eq!(balances_map.get(&sr.address).unwrap(), &initial_balance);
        assert_eq!(
            balances_map.get(&sr_stash.address).unwrap(),
            &initial_balance
        );
    }

    #[test]
    fn add_balances_ensure_zombie_account() {
        let mut spec_plain = chain_spec_test(ROCOCO_LOCAL_PLAIN_TESTING);

        let balances = spec_plain
            .pointer("/genesis/runtime/balances/balances")
            .unwrap();
        let balances_map = generate_balance_map(balances);

        let nodes: Vec<NodeSpec> = vec![];
        add_balances("/genesis/runtime", &mut spec_plain, &nodes, 12, 0);

        let new_balances = spec_plain
            .pointer("/genesis/runtime/balances/balances")
            .unwrap();

        let new_balances_map = generate_balance_map(new_balances);

        // sr and sr_stash keys exists
        assert!(new_balances_map.contains_key("5FTcLfwFc7ctvqp3RhbEig6UuHLHcHVRujuUm8r21wy4dAR8"));
        assert_eq!(new_balances_map.len(), balances_map.len() + 1);
    }

    #[test]
    fn add_balances_spec_without_balances() {
        let mut spec_plain = chain_spec_test(ROCOCO_LOCAL_PLAIN_TESTING);

        {
            let balances = spec_plain.pointer_mut("/genesis/runtime/balances").unwrap();
            *balances = json!(serde_json::Value::Null);
        }

        let mut name = String::from("luca");
        let initial_balance = 1_000_000_000_000_u128;
        let seed = format!("//{}{name}", name.remove(0).to_uppercase());
        let accounts = NodeAccounts {
            accounts: generators::generate_node_keys(&seed).unwrap(),
            seed,
        };
        let node = NodeSpec {
            name,
            accounts,
            initial_balance,
            ..Default::default()
        };

        let nodes = vec![node];
        add_balances("/genesis/runtime", &mut spec_plain, &nodes, 12, 0);

        let new_balances = spec_plain.pointer("/genesis/runtime/balances/balances");

        // assert 'balances' is not created
        assert_eq!(new_balances, None);
    }

    #[test]
    fn add_staking_works() {
        let mut chain_spec_json = chain_spec_with_stake();
        let mut name = String::from("luca");
        let initial_balance = 1_000_000_000_000_u128;
        let seed = format!("//{}{name}", name.remove(0).to_uppercase());
        let accounts = NodeAccounts {
            accounts: generators::generate_node_keys(&seed).unwrap(),
            seed,
        };
        let node = NodeSpec {
            name,
            accounts,
            initial_balance,
            ..Default::default()
        };

        let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();
        let min = get_staking_min(&pointer, &mut chain_spec_json);

        let nodes = vec![node];
        add_staking(&pointer, &mut chain_spec_json, &nodes, min);

        let new_staking = chain_spec_json
            .pointer("/genesis/runtimeGenesis/patch/staking")
            .unwrap();

        // stakers should be one (with the luca sr_stash accounts)
        let sr_stash = nodes[0].accounts.accounts.get("sr_stash").unwrap();
        assert_eq!(new_staking["stakers"][0][0], json!(sr_stash.address));
        // with the calculated minimal bound
        assert_eq!(new_staking["stakers"][0][2], json!(min));
        // and only one
        assert_eq!(new_staking["stakers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn adding_hrmp_channels_works() {
        let mut spec_plain = chain_spec_test(ROCOCO_LOCAL_PLAIN_TESTING);

        {
            let current_hrmp_channels = spec_plain
                .pointer("/genesis/runtime/hrmp/preopenHrmpChannels")
                .unwrap();
            // assert should be empty
            assert_eq!(current_hrmp_channels, &json!([]));
        }

        let para_100_101 = HrmpChannelConfigBuilder::new()
            .with_sender(100)
            .with_recipient(101)
            .build();
        let para_101_100 = HrmpChannelConfigBuilder::new()
            .with_sender(101)
            .with_recipient(100)
            .build();
        let channels = vec![para_100_101, para_101_100];

        add_hrmp_channels("/genesis/runtime", &mut spec_plain, &channels);
        let new_hrmp_channels = spec_plain
            .pointer("/genesis/runtime/hrmp/preopenHrmpChannels")
            .unwrap()
            .as_array()
            .unwrap();

        assert_eq!(new_hrmp_channels.len(), 2);
        assert_eq!(new_hrmp_channels.first().unwrap()[0], 100);
        assert_eq!(new_hrmp_channels.first().unwrap()[1], 101);
        assert_eq!(new_hrmp_channels.last().unwrap()[0], 101);
        assert_eq!(new_hrmp_channels.last().unwrap()[1], 100);
    }

    #[test]
    fn adding_hrmp_channels_to_an_spec_without_channels() {
        let mut spec_plain = chain_spec_test("./testing/rococo-local-plain.json");

        {
            let hrmp = spec_plain.pointer_mut("/genesis/runtime/hrmp").unwrap();
            *hrmp = json!(serde_json::Value::Null);
        }

        let para_100_101 = HrmpChannelConfigBuilder::new()
            .with_sender(100)
            .with_recipient(101)
            .build();
        let para_101_100 = HrmpChannelConfigBuilder::new()
            .with_sender(101)
            .with_recipient(100)
            .build();
        let channels = vec![para_100_101, para_101_100];

        add_hrmp_channels("/genesis/runtime", &mut spec_plain, &channels);
        let new_hrmp_channels = spec_plain.pointer("/genesis/runtime/hrmp/preopenHrmpChannels");

        // assert 'preopenHrmpChannels' is not created
        assert_eq!(new_hrmp_channels, None);
    }

    #[test]
    fn get_node_keys_works() {
        let mut name = String::from("luca");
        let seed = format!("//{}{name}", name.remove(0).to_uppercase());
        let accounts = NodeAccounts {
            accounts: generators::generate_node_keys(&seed).unwrap(),
            seed,
        };
        let node = NodeSpec {
            name,
            accounts,
            ..Default::default()
        };

        let sr = &node.accounts.accounts["sr"];
        let keys = [
            ("babe".into(), sr.address.clone()),
            ("im_online".into(), sr.address.clone()),
            ("parachain_validator".into(), sr.address.clone()),
            ("authority_discovery".into(), sr.address.clone()),
            ("para_validator".into(), sr.address.clone()),
            ("para_assignment".into(), sr.address.clone()),
            ("aura".into(), sr.address.clone()),
            ("nimbus".into(), sr.address.clone()),
            ("vrf".into(), sr.address.clone()),
            (
                "grandpa".into(),
                node.accounts.accounts["ed"].address.clone(),
            ),
            ("beefy".into(), node.accounts.accounts["ec"].address.clone()),
            ("eth".into(), node.accounts.accounts["eth"].address.clone()),
        ]
        .into();

        // Stash
        let sr_stash = &node.accounts.accounts["sr_stash"];
        let node_key = get_node_keys(&node, SessionKeyType::Stash, false);
        assert_eq!(node_key.0, sr_stash.address);
        assert_eq!(node_key.1, sr_stash.address);
        assert_eq!(node_key.2, keys);
        // Non-stash
        let node_key = get_node_keys(&node, SessionKeyType::Default, false);
        assert_eq!(node_key.0, sr.address);
        assert_eq!(node_key.1, sr.address);
        assert_eq!(node_key.2, keys);
    }

    #[test]
    fn get_node_keys_supports_asset_hub_polkadot() {
        let mut name = String::from("luca");
        let seed = format!("//{}{name}", name.remove(0).to_uppercase());
        let accounts = NodeAccounts {
            accounts: generators::generate_node_keys(&seed).unwrap(),
            seed,
        };
        let node = NodeSpec {
            name,
            accounts,
            ..Default::default()
        };

        let node_key = get_node_keys(&node, SessionKeyType::default(), false);
        assert_eq!(node_key.2["aura"], node.accounts.accounts["sr"].address);

        let node_key = get_node_keys(&node, SessionKeyType::default(), true);
        assert_eq!(node_key.2["aura"], node.accounts.accounts["ed"].address);
    }
}
