use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use configuration::{
    types::{AssetLocation, Chain, ChainSpecRuntime, JsonOverrides, ParaId},
    HrmpChannelConfig,
};
use provider::{
    constants::NODE_CONFIG_DIR,
    types::{GenerateFileCommand, GenerateFilesOptions, TransferedFile},
    DynNamespace, ProviderError,
};
use sc_chain_spec::{GenericChainSpec, GenesisConfigBuilderRuntimeCaller};
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

// TODO: (javier) move to state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Context {
    Relay,
    Para { relay_chain: Chain, para_id: ParaId },
}

/// Posible chain-spec formats
#[derive(Debug, Clone, Copy)]
enum ChainSpecFormat {
    Plain,
    Raw,
}
/// Key types to replace in spec
#[derive(Debug, Clone, Copy)]
enum KeyType {
    Session,
    Aura,
    Grandpa,
}

#[derive(Debug, Clone, Copy, Default)]
enum SessionKeyType {
    // Default derivarion (e.g `//`)
    #[default]
    Default,
    // Stash detivarion (e.g `//<name>/stash`)
    Stash,
    // EVM session type
    Evm,
}

type MaybeExpectedPath = Option<PathBuf>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandInContext {
    Local(String, MaybeExpectedPath),
    Remote(String, MaybeExpectedPath),
}

impl CommandInContext {
    fn cmd(&self) -> &str {
        match self {
            CommandInContext::Local(cmd, _) | CommandInContext::Remote(cmd, _) => cmd.as_ref(),
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

/// Presets to check if is not set by the user.
/// We check if the preset is valid for the runtime in order
/// and if non of them are preset we fallback to the `default config`.
const DEFAULT_PRESETS_TO_CHECK: [&str; 3] = ["local_testnet", "development", "dev"];

/// Chain-spec builder representation
///
/// Multiple options are supported, and the current order is:
/// IF [`asset_location`] is _some_ -> Use this chain_spec by copying the file from [`AssetLocation`]
/// ELSE IF [`runtime_location`] is _some_ -> generate the chain-spec using the sc-chain-spec builder.
/// ELSE -> Fallback to use the `default` or customized cmd.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSpec {
    // Name of the spec file, most of the times could be the same as the chain_name. (e.g rococo-local)
    chain_spec_name: String,
    // Location of the chain-spec to use
    asset_location: Option<AssetLocation>,
    // Location of the runtime to use
    runtime: Option<ChainSpecRuntime>,
    maybe_plain_path: Option<PathBuf>,
    chain_name: Option<String>,
    raw_path: Option<PathBuf>,
    // The binary to build the chain-spec
    command: Option<CommandInContext>,
    // Imgae to use for build the chain-spec
    image: Option<String>,
    // Contex of the network (e.g relay or para)
    context: Context,
}

impl ChainSpec {
    pub(crate) fn new(chain_spec_name: impl Into<String>, context: Context) -> Self {
        Self {
            chain_spec_name: chain_spec_name.into(),
            chain_name: None,
            maybe_plain_path: None,
            asset_location: None,
            runtime: None,
            raw_path: None,
            command: None,
            image: None,
            context,
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

    pub(crate) fn asset_location(mut self, location: AssetLocation) -> Self {
        self.asset_location = Some(location);
        self
    }

    pub(crate) fn runtime(mut self, chain_spec_runtime: ChainSpecRuntime) -> Self {
        self.runtime = Some(chain_spec_runtime);
        self
    }

    pub(crate) fn command(
        mut self,
        command: impl Into<String>,
        is_local: bool,
        expected_path: Option<&str>,
    ) -> Self {
        let maybe_expected_path = expected_path.map(PathBuf::from);
        let cmd = if is_local {
            CommandInContext::Local(command.into(), maybe_expected_path)
        } else {
            CommandInContext::Remote(command.into(), maybe_expected_path)
        };
        self.command = Some(cmd);
        self
    }

    pub(crate) fn image(mut self, image: Option<String>) -> Self {
        self.image = image;
        self
    }

    /// Build the chain-spec
    ///
    /// Chain spec generation flow:
    /// if chain_spec_path is set -> use this chain_spec
    /// else if runtime_path is set and cmd is compatible with chain-spec-builder -> use the chain-spec-builder
    /// else if chain_spec_command is set -> use this cmd for generate the chain_spec
    /// else -> use the default command.
    pub async fn build<'a, T>(
        &mut self,
        ns: &DynNamespace,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        if self.asset_location.is_none() && self.command.is_none() && self.runtime.is_none() {
            return Err(GeneratorError::ChainSpecGeneration(
                "Can not build the chain spec without set the command, asset_location or runtime"
                    .to_string(),
            ));
        }

        let maybe_plain_spec_path = PathBuf::from(format!("{}-plain.json", self.chain_spec_name));

        // if asset_location is some, then copy the asset to the `base_dir` of the ns with the name `<name>-plain.json`
        if let Some(location) = self.asset_location.as_ref() {
            let maybe_plain_spec_full_path = scoped_fs.full_path(maybe_plain_spec_path.as_path());
            location
                .dump_asset(maybe_plain_spec_full_path)
                .await
                .map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "Error {e} dumping location {location:?}"
                    ))
                })?;
        } else if let Some(runtime) = self.runtime.as_ref() {
            trace!(
                "Creating chain-spec with runtime from localtion: {}",
                runtime.location
            );
            // First dump the runtime into the ns scoped fs, since we want to easily reproduce
            let runtime_file_name = PathBuf::from(format!("{}-runtime.wasm", self.chain_spec_name));
            let runtime_path_ns = scoped_fs.full_path(runtime_file_name.as_path());
            runtime
                .location
                .dump_asset(runtime_path_ns)
                .await
                .map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "Error {e} dumping location {:?}",
                        runtime.location
                    ))
                })?;

            // list the presets to check if match with the supplied one or one of the defaults
            let runtime_code = scoped_fs.read(runtime_file_name.as_path()).await?;

            let caller: GenesisConfigBuilderRuntimeCaller =
                GenesisConfigBuilderRuntimeCaller::new(&runtime_code[..]);
            let presets = caller.preset_names().map_err(|e| {
                GeneratorError::ChainSpecGeneration(format!(
                    "getting default config from runtime should work: {e}"
                ))
            })?;

            // check the preset to use with this priorities:
            // - IF user provide a preset (and if present) use it
            // - else (user don't provide preset or the provided one isn't preset)
            //     check the [`DEFAULT_PRESETS_TO_CHECK`] in order to find one valid
            // - If we can't find any valid preset use the `default config` from the runtime

            let preset_to_check = if let Some(preset) = &runtime.preset {
                [vec![preset.as_str()], DEFAULT_PRESETS_TO_CHECK.to_vec()].concat()
            } else {
                DEFAULT_PRESETS_TO_CHECK.to_vec()
            };
            let preset = preset_to_check
                .iter()
                .find(|preset| presets.iter().any(|item| item == *preset));

            trace!("presets: {:?} - preset to use: {:?}", presets, preset);
            let builder = if let Some(preset) = preset {
                GenericChainSpec::<()>::builder(&runtime_code[..], ())
                    .with_genesis_config_preset_name(preset)
            } else {
                // default config
                let default_config = caller.get_default_config().map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "getting default config from runtime should work: {e}"
                    ))
                })?;

                GenericChainSpec::<()>::builder(&runtime_code[..], ())
                    .with_genesis_config(default_config)
            };

            let builder = if let Context::Para {
                relay_chain: _,
                para_id: _,
            } = &self.context
            {
                builder.with_id(self.chain_spec_name())
            } else {
                builder
            };

            let builder = if let Some(chain_name) = self.chain_name.as_ref() {
                builder.with_name(chain_name)
            } else {
                builder
            };

            let chain_spec = builder.build();

            let contents = chain_spec.as_json(false).map_err(|e| {
                GeneratorError::ChainSpecGeneration(format!(
                    "getting chain-spec as json should work, err: {e}"
                ))
            })?;

            scoped_fs.write(&maybe_plain_spec_path, contents).await?;
        } else {
            trace!("Creating chain-spec with command");
            // we should create the chain-spec using command.
            let mut replacement_value = String::default();
            if let Some(chain_name) = self.chain_name.as_ref() {
                if !chain_name.is_empty() {
                    replacement_value.clone_from(chain_name);
                }
            };

            // SAFETY: we ensure that command is some with the first check of the fn
            // default as empty
            let sanitized_cmd = if replacement_value.is_empty() {
                // we need to remove the `--chain` flag
                self.command.as_ref().unwrap().cmd().replace("--chain", "")
            } else {
                self.command.as_ref().unwrap().cmd().to_owned()
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
            if let Some(cmd) = &self.command {
                match cmd {
                    CommandInContext::Local(_, expected_path) => {
                        build_locally(generate_command, scoped_fs, expected_path.as_deref()).await?
                    },
                    CommandInContext::Remote(_, expected_path) => {
                        let options = GenerateFilesOptions::new(
                            vec![generate_command],
                            self.image.clone(),
                            expected_path.clone(),
                        );
                        ns.generate_files(options).await?;
                    },
                }
            }
        }

        // check if the _generated_ spec is in raw mode.
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
        relay_chain_id: Option<Chain>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        warn!("Building raw version from {:?}", self);
        // raw path already set, no more work to do here...
        let None = self.raw_path else {
            return Ok(());
        };

        // expected raw path
        let raw_spec_path = PathBuf::from(format!("{}.json", self.chain_spec_name));

        // workaround, IFF the cmd is `polkadot-omni-node` we rely on the GenericChainSpec always
        let is_omni_node = if let Some(cmd) = self.command.as_ref() {
            // chains created with omni-node or pop-cli
            cmd.cmd().contains("omni-node") || cmd.cmd().contains("pop")
        } else {
            false
        };

        if (self.runtime.is_some() && self.asset_location.is_none()) || is_omni_node {
            match self
                .try_build_raw_with_generic(
                    scoped_fs,
                    relay_chain_id.clone(),
                    raw_spec_path.as_path(),
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(err) => {
                    if Self::should_retry_with_command(&err) && self.command.is_some() {
                        warn!(
                            "GenericChainSpec raw generation failed ({}). Falling back to command execution.",
                            err
                        );
                    } else {
                        return Err(err);
                    }
                },
            }
        }

        self.build_raw_with_command(ns, scoped_fs, raw_spec_path, relay_chain_id)
            .await?;

        Ok(())
    }

    async fn try_build_raw_with_generic<'a, T>(
        &mut self,
        scoped_fs: &ScopedFilesystem<'a, T>,
        relay_chain_id: Option<Chain>,
        raw_spec_path: &Path,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        // `build_raw` is always called after `build`, so `maybe_plain_path` must be set at this point
        let (json_content, _) = self.read_spec(scoped_fs).await?;
        let json_bytes: Vec<u8> = json_content.as_bytes().into();
        let chain_spec = GenericChainSpec::<()>::from_json_bytes(json_bytes).map_err(|e| {
            GeneratorError::ChainSpecGeneration(format!(
                "Error loading chain-spec from json_bytes, err: {e}"
            ))
        })?;

        let contents = chain_spec.as_json(true).map_err(|e| {
            GeneratorError::ChainSpecGeneration(format!(
                "getting chain-spec as json should work, err: {e}"
            ))
        })?;

        let contents = if let Context::Para {
            relay_chain: _,
            para_id,
        } = &self.context
        {
            let mut contents_json: serde_json::Value =
                serde_json::from_str(&contents).map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "getting chain-spec as json should work, err: {e}"
                    ))
                })?;

            if contents_json["relay_chain"].is_null() {
                contents_json["relay_chain"] = json!(relay_chain_id);
            }

            if contents_json["para_id"].is_null() {
                contents_json["para_id"] = json!(para_id);
            }

            serde_json::to_string_pretty(&contents_json).map_err(|e| {
                GeneratorError::ChainSpecGeneration(format!(
                    "getting chain-spec json as pretty string should work, err: {e}"
                ))
            })?
        } else {
            contents
        };

        self.raw_path = Some(raw_spec_path.to_path_buf());
        self.write_spec(scoped_fs, contents).await?;

        Ok(())
    }

    async fn build_raw_with_command<'a, T>(
        &mut self,
        ns: &DynNamespace,
        scoped_fs: &ScopedFilesystem<'a, T>,
        raw_spec_path: PathBuf,
        relay_chain_id: Option<Chain>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        // fallback to use _cmd_ for raw creation
        let temp_name = format!(
            "temp-build-raw-{}-{}",
            self.chain_spec_name,
            rand::random::<u8>()
        );

        let cmd = self
            .command
            .as_ref()
            .ok_or(GeneratorError::ChainSpecGeneration(
                "Invalid command".into(),
            ))?;
        let maybe_plain_path =
            self.maybe_plain_path
                .as_ref()
                .ok_or(GeneratorError::ChainSpecGeneration(
                    "Invalid plain path".into(),
                ))?;

        // TODO: we should get the full path from the scoped filesystem
        let chain_spec_path_local = format!(
            "{}/{}",
            ns.base_dir().to_string_lossy(),
            maybe_plain_path.display()
        );
        // Remote path to be injected
        let chain_spec_path_in_pod = format!("{}/{}", NODE_CONFIG_DIR, maybe_plain_path.display());
        // Path in the context of the node, this can be different in the context of the providers (e.g native)
        let chain_spec_path_in_args = if matches!(self.command, Some(CommandInContext::Local(_, _)))
        {
            chain_spec_path_local.clone()
        } else if ns.capabilities().prefix_with_full_path {
            // In native
            format!(
                "{}/{}{}",
                ns.base_dir().to_string_lossy(),
                &temp_name,
                &chain_spec_path_in_pod
            )
        } else {
            chain_spec_path_in_pod.clone()
        };

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

        if let Some(cmd) = &self.command {
            match cmd {
                CommandInContext::Local(_, expected_path) => {
                    build_locally(generate_command, scoped_fs, expected_path.as_deref()).await?
                },
                CommandInContext::Remote(_, expected_path) => {
                    let options = GenerateFilesOptions::with_files(
                        vec![generate_command],
                        self.image.clone(),
                        &[TransferedFile::new(
                            chain_spec_path_local,
                            chain_spec_path_in_pod,
                        )],
                        expected_path.clone(),
                    )
                    .temp_name(temp_name);
                    trace!("calling generate_files with options: {:#?}", options);
                    ns.generate_files(options).await?;
                },
            }
        }

        self.raw_path = Some(raw_spec_path.clone());
        self.ensure_para_fields_in_raw(scoped_fs, relay_chain_id)
            .await?;

        Ok(())
    }

    async fn ensure_para_fields_in_raw<'a, T>(
        &mut self,
        scoped_fs: &ScopedFilesystem<'a, T>,
        relay_chain_id: Option<Chain>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        if let Context::Para {
            relay_chain: _,
            para_id,
        } = &self.context
        {
            let (content, _) = self.read_spec(scoped_fs).await?;
            let mut chain_spec_json: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "getting chain-spec as json should work, err: {e}"
                    ))
                })?;

            let mut needs_write = false;

            if chain_spec_json["relay_chain"].is_null() {
                chain_spec_json["relay_chain"] = json!(relay_chain_id);
                needs_write = true;
            }

            if chain_spec_json["para_id"].is_null() {
                chain_spec_json["para_id"] = json!(para_id);
                needs_write = true;
            }

            if needs_write {
                let contents = serde_json::to_string_pretty(&chain_spec_json).map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "getting chain-spec json as pretty string should work, err: {e}"
                    ))
                })?;
                self.write_spec(scoped_fs, contents).await?;
            }
        }

        Ok(())
    }

    fn should_retry_with_command(err: &GeneratorError) -> bool {
        match err {
            GeneratorError::ChainSpecGeneration(msg) => {
                let msg_lower = msg.to_lowercase();
                msg_lower.contains("genesisbuilder_get_preset") || msg_lower.contains("_get_preset")
            },
            _ => false,
        }
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

    pub async fn override_raw_spec<'a, T>(
        &mut self,
        scoped_fs: &ScopedFilesystem<'a, T>,
        raw_spec_overrides: &JsonOverrides,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        // first ensure we have the raw version of the chain-spec
        let Some(_) = self.raw_path else {
            return Err(GeneratorError::OverridingRawSpec(String::from(
                "Raw path should be set at this point.",
            )));
        };

        let (content, _) = self.read_spec(scoped_fs).await?;

        // read overrides to json value
        let override_content: serde_json::Value = raw_spec_overrides.get().await.map_err(|_| {
            GeneratorError::OverridingRawSpec(format!(
                "Can not parse raw_spec_override contents as json: {raw_spec_overrides}"
            ))
        })?;

        // read spec to json value
        let mut chain_spec_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|_| {
                GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into())
            })?;

        // merge overrides with existing spec
        merge(&mut chain_spec_json, &override_content);

        // save changes
        let overrided_content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
            GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
        })?;
        self.write_spec(scoped_fs, overrided_content).await?;

        Ok(())
    }

    pub fn raw_path(&self) -> Option<&Path> {
        self.raw_path.as_deref()
    }

    pub fn set_asset_location(&mut self, location: AssetLocation) {
        self.asset_location = Some(location)
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

            clear_authorities(&pointer, &mut chain_spec_json, &self.context);

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
            clear_authorities(&pointer, &mut chain_spec_json, &self.context);

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
}

type GenesisNodeKey = (String, String, HashMap<String, String>);

async fn build_locally<'a, T>(
    generate_command: GenerateFileCommand,
    scoped_fs: &ScopedFilesystem<'a, T>,
    maybe_output: Option<&Path>,
) -> Result<(), GeneratorError>
where
    T: FileSystem,
{
    // generate_command.

    let result = Command::new(generate_command.program.clone())
        .args(generate_command.args.clone())
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
        let raw_output = if let Some(output_path) = maybe_output {
            tokio::fs::read(output_path).await.map_err(|err| {
                GeneratorError::ChainSpecGeneration(format!(
                    "Error reading output file at {}: {}",
                    output_path.display(),
                    err
                ))
            })?
        } else {
            result.stdout
        };
        scoped_fs
            .write(
                generate_command.local_output_path,
                String::from_utf8_lossy(&raw_output).to_string(),
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

        let paras = val
            .pointer_mut(paras_pointer)
            .ok_or(anyhow!("paras pointer should be valid {paras_pointer:?} "))?;
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
        .ok_or_else(|| anyhow!("Invalid override value: {overrides:?}"))?;
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
        .ok_or_else(|| anyhow!("Invalid override value: {overrides:?}"))?;
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
                .ok_or_else(|| anyhow!("Invalid override value: {overrides:?}"))?;
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

fn clear_authorities(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    ctx: &Context,
) {
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
        if val.get("staking").is_some() && ctx == &Context::Relay {
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
        clear_authorities(&pointer, &mut chain_spec_json, &Context::Relay);

        let validator_count = chain_spec_json
            .pointer(&format!("{pointer}/staking/validatorCount"))
            .unwrap();
        assert_eq!(validator_count, &json!(500));
    }

    #[test]
    fn dev_stakers_override_count_works() {
        let mut chain_spec_json = chain_spec_with_stake();

        let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();
        clear_authorities(&pointer, &mut chain_spec_json, &Context::Relay);

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
