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
use serde_json::json;
use support::{constants::THIS_IS_A_BUG, fs::FileSystem, replacer::apply_replacements};
use tokio::process::Command;
use tracing::{debug, trace, warn};

use super::errors::GeneratorError;
use crate::{
    network_spec::{node::NodeSpec, parachain::ParachainSpec, relaychain::RelaychainSpec},
    ScopedFilesystem,
};

// TODO: (javier) move to state
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum CommandInContext {
    Local(String),
    Remote(String),
}

impl CommandInContext {
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

#[derive(Debug, Clone)]
pub struct ChainSpec {
    // Name of the spec file, most of the times could be the same as the chain_name. (e.g rococo-local)
    chain_spec_name: String,
    asset_location: Option<AssetLocation>,
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

    pub(crate) fn command(mut self, command: impl Into<String>, is_local: bool) -> Self {
        let cmd = if is_local {
            CommandInContext::Local(command.into())
        } else {
            CommandInContext::Remote(command.into())
        };
        self.command = Some(cmd);
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
        // TODO: Move this to state builder.
        if self.asset_location.is_none() && self.command.is_none() {
            return Err(GeneratorError::ChainSpecGeneration(
                "Can not build the chain spec without set the command or asset_location"
                    .to_string(),
            ));
        }

        let maybe_plain_spec_path = PathBuf::from(format!("{}-plain.json", self.chain_spec_name));
        // if we have a path, copy to the base_dir of the ns with the name `<name>-plain.json`
        if let Some(location) = self.asset_location.as_ref() {
            match location {
                AssetLocation::FilePath(path) => {
                    let file_to_transfer =
                        TransferedFile::new(path.clone(), maybe_plain_spec_path.clone());

                    scoped_fs
                        .copy_files(vec![&file_to_transfer])
                        .await
                        .map_err(|_| {
                            GeneratorError::ChainSpecGeneration(format!(
                                "Error copying file: {file_to_transfer}"
                            ))
                        })?;
                },
                AssetLocation::Url(_url) => todo!(),
            }
        } else {
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
                //.as_ref().unwrap().replace("--chain", "")
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
            if let Some(CommandInContext::Local(_)) = self.command {
                // local
                build_locally(generate_command, scoped_fs).await?;
            } else {
                // remote
                let options = GenerateFilesOptions::new(vec![generate_command], self.image.clone());
                ns.generate_files(options).await?;
            }
        }

        if is_raw(maybe_plain_spec_path.clone(), scoped_fs).await? {
            let spec_path = PathBuf::from(format!("{}.json", self.chain_spec_name));
            let tf_file = TransferedFile::new(
                &PathBuf::from_iter([ns.base_dir(), &maybe_plain_spec_path]),
                &spec_path,
            );
            scoped_fs.copy_files(vec![&tf_file]).await.map_err(|e| {
                GeneratorError::ChainSpecGeneration(format!(
                    "Error copying file: {}, err: {}",
                    tf_file, e
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
        let chain_spec_path_in_args = if matches!(self.command, Some(CommandInContext::Local(_))) {
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

        if let Some(CommandInContext::Local(_)) = self.command {
            // local
            build_locally(generate_command, scoped_fs).await?;
        } else {
            // remote
            let options = GenerateFilesOptions::with_files(
                vec![generate_command],
                self.image.clone(),
                &[TransferedFile::new(
                    chain_spec_path_local,
                    chain_spec_path_in_pod,
                )],
            )
            .temp_name(temp_name);
            trace!("calling generate_files with options: {:#?}", options);
            ns.generate_files(options).await?;
        }

        self.raw_path = Some(raw_spec_path);

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
                if let Some(genesis) = chain_spec_json.pointer_mut(&pointer) {
                    merge(genesis, overrides);
                }
            }

            clear_authorities(&pointer, &mut chain_spec_json);

            // Get validators to add as authorities
            let validators: Vec<&NodeSpec> = para
                .collators
                .iter()
                .filter(|node| node.is_validator)
                .collect();

            // check chain key types
            if chain_spec_json
                .pointer(&format!("{}/session", pointer))
                .is_some()
            {
                add_authorities(&pointer, &mut chain_spec_json, &validators, false);
            } else if chain_spec_json
                .pointer(&format!("{}/aura", pointer))
                .is_some()
            {
                add_aura_authorities(&pointer, &mut chain_spec_json, &validators, KeyType::Aura);
                // await addParaCustom(chainSpecFullPathPlain, node);
            } else {
                warn!("Can't customize keys, not `session` or `aura` find in the chain-spec file");
            };

            // Add nodes to collator
            let invulnerables: Vec<&NodeSpec> = para
                .collators
                .iter()
                .filter(|node| node.is_invulnerable)
                .collect();

            add_collator_selection(&pointer, &mut chain_spec_json, &invulnerables);

            // override `parachainInfo/parachainId`
            override_parachain_info(&pointer, &mut chain_spec_json, para.id);

            // write spec
            let content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
                GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
            })?;
            self.write_spec(scoped_fs, content).await?;
        } else {
            // TODO: add a warning here
            todo!();
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
                if let Some(patch_section) = chain_spec_json.pointer_mut(&pointer) {
                    merge(patch_section, overrides);
                }
            }

            // Clear authorities
            clear_authorities(&pointer, &mut chain_spec_json);

            // add balances
            add_balances(
                &pointer,
                &mut chain_spec_json,
                &relaychain.nodes,
                token_decimals,
                0,
            );

            // Get validators to add as authorities
            let validators: Vec<&NodeSpec> = relaychain
                .nodes
                .iter()
                .filter(|node| node.is_validator)
                .collect();

            // check chain key types
            if chain_spec_json
                .pointer(&format!("{}/session", pointer))
                .is_some()
            {
                add_authorities(&pointer, &mut chain_spec_json, &validators, true);
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
            // - manage session/aura for keys ( Javier think is done!)
            // - staking
            // - nominators
            // - hrmp_channels

            // write spec
            let content = serde_json::to_string_pretty(&chain_spec_json).map_err(|_| {
                GeneratorError::ChainSpecGeneration("can not parse chain-spec value as json".into())
            })?;
            self.write_spec(scoped_fs, content).await?;
        } else {
            // TODO: add a warning here
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

// Merge `patch_section` with `overrides`.
fn merge(patch_section: &mut serde_json::Value, overrides: &serde_json::Value) {
    if let (Some(genesis_obj), Some(overrides_obj)) =
        (patch_section.as_object_mut(), overrides.as_object())
    {
        for overrides_key in overrides_obj.keys() {
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
                        *genesis_value = overrides_value.clone();
                    },
                    _ => {},
                }
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

        // clear staking
        if val.get("staking").is_some() {
            val["staking"]["stakers"] = json!([]);
            val["staking"]["invulnerables"] = json!([]);
            val["staking"]["validatorCount"] = json!(0);
        }
    } else {
        unreachable!("pointer to runtime config should be valid!")
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
            let balance = std::cmp::max(node.initial_balance, staking_min);
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

fn get_node_keys(node: &NodeSpec, use_stash: bool) -> GenesisNodeKey {
    let sr_account = node.accounts.accounts.get("sr").unwrap();
    let sr_stash = node.accounts.accounts.get("sr_stash").unwrap();
    let ed_account = node.accounts.accounts.get("ed").unwrap();
    let ec_account = node.accounts.accounts.get("ec").unwrap();
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
        keys.insert(k.to_string(), sr_account.address.clone());
    }

    keys.insert("grandpa".to_string(), ed_account.address.clone());
    keys.insert("beefy".to_string(), ec_account.address.clone());

    let account_to_use = if use_stash { sr_stash } else { sr_account };
    (
        account_to_use.address.clone(),
        account_to_use.address.clone(),
        keys,
    )
}
fn add_authorities(
    runtime_config_ptr: &str,
    chain_spec_json: &mut serde_json::Value,
    nodes: &[&NodeSpec],
    use_stash: bool,
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
        let keys: Vec<GenesisNodeKey> = nodes
            .iter()
            .map(|node| get_node_keys(node, use_stash))
            .collect();
        val["session"]["keys"] = json!(keys);
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
        val["aura"]["authorities"] = json!(keys);
    } else {
        unreachable!("pointer to runtime config should be valid!")
    }
}
// TODO: (team)

// fn add_staking() {}
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
) {
    if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
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
        assert_eq!(new_hrmp_channels.first().unwrap()["sender"], 100);
        assert_eq!(new_hrmp_channels.first().unwrap()["recipient"], 101);
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
}
