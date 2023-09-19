use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ScopedFilesystem;
use crate::network_spec::node::NodeSpec;
use crate::network_spec::relaychain::RelaychainSpec;

use super::errors::GeneratorError;
use super::para_artifact::ParaArtifact;
use configuration::HrmpChannelConfig;
use configuration::types::AssetLocation;
use provider::{ProviderError, DynNamespace};
use provider::types::{TransferedFile, GenerateFileCommand, GenerateFilesOptions};
use serde_json::json;
use support::fs::FileSystem;

enum ChainSpecFormat {
    Plain,
    Raw
}

enum  KeyType {
    Session,
    Aura,
    Grandpa
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
    command: Option<String>,
    // full command to build the spec, we will use as provided
    build_command: Option<String>
}


impl ChainSpec {
    pub(crate) fn new(chain_spec_name: impl Into<String>) -> Self {
        Self {
            chain_spec_name: chain_spec_name.into(),
            build_command: None,
            chain_name: None,
            maybe_plain_path: None,
            asset_location: None,
            raw_path: None,
            command: None,
        }
    }

    pub(crate) fn chain_name(mut self, chain_name: impl Into<String>) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    pub(crate) fn asset_location(mut self, location: AssetLocation) -> Self {
        self.asset_location = Some(location);
        self
    }

    pub(crate) fn commad(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    // pub(crate) fn with_chain(chain_name: impl Into<String>, command: impl Into<String>) -> Self {
    //     let name = chain_name.into();
    //     let build_command = format!(
    //         "{} build-spec {} --disable-default-bootnode",
    //         command.into(),
    //         &name
    //     );

    //     Self {
    //         build_command: Some(build_command),
    //         chain_name: Some(name),
    //         maybe_plain_path: None,
    //         raw_path: None,
    //     }
    // }


    /// Build the chain-spec
    ///
    pub async fn build<'a, T>(&mut self, ns: &DynNamespace, scoped_fs: &ScopedFilesystem<'a, T>) -> Result<(), GeneratorError>
    where T: FileSystem
    {
        // TODO: Move this to state builder.
        if self.asset_location.is_none() && self.command.is_none() {
            return Err(GeneratorError::ChainSpecGeneration("Can not build the chain spec without set the command or asset_location".to_string()));
        }

        let maybe_plain_spec_path = PathBuf::from(format!("{}-plain.json", self.chain_spec_name));
        // if we have a path, copy to the base_dir of the ns with the name `<name>-plain.json`
        if let Some(location) = self.asset_location.as_ref() {
            match location {
                AssetLocation::FilePath(path) => {
                    let file_to_transfer = TransferedFile {
                        local_path: path.clone(),
                        remote_path: maybe_plain_spec_path.clone()
                    };

                    scoped_fs.copy_files(vec![&file_to_transfer])
                    .await
                    .map_err(|_| GeneratorError::ChainSpecGeneration(format!("Error copying file: {file_to_transfer}")))?;
                },
                AssetLocation::Url(_url) => todo!()
            }
        } else {
            // we should create the chain-spec using command.
            // SAFETY: we ensure that command is some with the first check of the fn
            let cmd = self.command.as_ref().unwrap();
            let mut args: Vec<String> = vec!["build-spec".into()];
            if let Some(chain_name) = self.chain_name.as_ref() {
                args.push("--chain".into());
                args.push(chain_name.clone());
            }
            args.push("--disable-default-bootnode".into());

            let generate_command = GenerateFileCommand::new(cmd.as_str(), maybe_plain_spec_path.clone()).args(args);
            let options = GenerateFilesOptions::new(vec![generate_command]);
            ns.generate_files(options).await?;
        }


        if is_raw(maybe_plain_spec_path.clone(), scoped_fs).await? {
            self.raw_path = Some(maybe_plain_spec_path);
        } else {
            self.maybe_plain_path = Some(maybe_plain_spec_path);
        }
        Ok(())
    }

    pub async fn build_raw(&mut self, ns: &DynNamespace) -> Result<(), GeneratorError>
    {
        let None = self.raw_path else {
            return Ok(());
        };
        // build raw
        let raw_spec_path = PathBuf::from(format!("{}.json", self.chain_spec_name));
        let cmd = self.command.as_ref().ok_or(GeneratorError::ChainSpecGeneration("Invalid command".into()))?;
        let maybe_plain_path = self.maybe_plain_path.as_ref().ok_or(GeneratorError::ChainSpecGeneration("Invalid plain path".into()))?;
        let args: Vec<String> = vec![
            "build-spec".into(),
            "--chain".into(),
            // TODO: we should get the full path from the scoped filesystem
            format!("{}/{}", ns.base_dir(), maybe_plain_path.display().to_string()),
            "--raw".into(),
            "--disable-default-bootnode".into(),
        ];


        let generate_command = GenerateFileCommand::new(cmd, raw_spec_path.clone()).args(args);
        let options = GenerateFilesOptions::new(vec![generate_command]);
        ns.generate_files(options).await?;

        self.raw_path = Some(raw_spec_path);

        Ok(())
    }

    pub fn raw_path(&self) -> Option<&Path> {
        self.raw_path.as_deref()
    }

    async fn read_spec<'a, T>(&self, scoped_fs: &ScopedFilesystem<'a, T>) -> Result<(String, ChainSpecFormat), GeneratorError>
    where T: FileSystem
    {
        let (path, format) = match (self.maybe_plain_path.as_ref(), self.raw_path.as_ref()) {
            (Some(path), None) => (path, ChainSpecFormat::Plain),
            (None, Some(path)) => (path, ChainSpecFormat::Raw),
            (Some(_), Some(path)) => {
                // if we have both paths return the raw
                (path, ChainSpecFormat::Raw)
            },
            (None, None) => unreachable!()
        };

        let content = scoped_fs.read_to_string(path.clone()).await
        .map_err(|_| GeneratorError::ChainSpecGeneration(format!("Can not read chain-spec from {}", path.to_string_lossy() )))?;

        Ok((content, format))
    }

    pub async fn customize_relay<'a, T>(&self, relaychain: &RelaychainSpec, _hrmp_channels: &Vec<HrmpChannelConfig>, _para_artifacts: Vec<ParaArtifact>, scoped_fs: &ScopedFilesystem<'a, T>) -> Result<(), GeneratorError>
    where T: FileSystem
    {
        let (content, format) = self.read_spec(scoped_fs).await?;
        let mut chain_spec_json: serde_json::Value = serde_json::from_str(&content).map_err(|_| GeneratorError::ChainSpecGeneration("Can not parse chain-spec as json".into()))?;

        if let ChainSpecFormat::Plain = format {
            // get the config pointer
            let pointer = get_runtime_config_pointer(&chain_spec_json).unwrap();
            // make genesis overrides first.

            println!("{:#?}", chain_spec_json.pointer(format!("{}/session/keys",pointer).as_str()));
            // Clear authorities
            clear_authorities(&pointer, &mut chain_spec_json);

            println!("{:#?}", chain_spec_json.pointer(format!("{}/session/keys",pointer).as_str()));

            println!("BALANCES");
            println!("{:#?}", chain_spec_json.pointer(format!("{}/balances",pointer).as_str()));
            // add balances
            add_balances(&pointer, &mut chain_spec_json, &relaychain.nodes, 0);

            println!("{:#?}", chain_spec_json.pointer(format!("{}/balances",pointer).as_str()));

            // Get validators to add as authorities
            let validators: Vec<&NodeSpec> = relaychain.nodes.iter().filter(|node| node.is_validator).collect();

            // check chain key types
            let _key_type = if let Some(_) = chain_spec_json.pointer(&format!("{}/session",pointer)) {
                add_authorities(&pointer, &mut chain_spec_json, &validators, KeyType::Session);
                KeyType::Session
            } else {
                KeyType::Aura
            };

            // staking && nominators

            //add_hrmp_channels

            println!("KEYS");
            println!("{:#?}", chain_spec_json.pointer(format!("{}/session/keys",pointer).as_str()));


            // TODO:
            // - manage session/aura for keys
            // - staking
            // - nominators
            // - hrmp_channels
        } else {
            // TODO: add a warning here
        }
        Ok(())
    }

}

type GenesisNodeKey = (String, String, HashMap<String, String>);

async fn is_raw<'a, T>(file: PathBuf, scoped_fs: &ScopedFilesystem<'a, T>) -> Result<bool, ProviderError>
where T: FileSystem
{

    let content = scoped_fs.read_to_string(file).await?;
    let chain_spec_json: serde_json::Value = serde_json::from_str(&content).unwrap();

    Ok(chain_spec_json.pointer("genesis/raw/top").is_some())
}

    // Internal Chain-spec customizations

    fn get_runtime_config_pointer(chain_spec_json: &serde_json::Value) ->  Result<String, ()> {
        // runtime_genesis_config is no longer in ChainSpec after rococo runtime rework (refer to: https://github.com/paritytech/polkadot-sdk/pull/1256)
        // ChainSpec may contain a RuntimeGenesisConfigPatch
        let pointers = [
            "/genesis/runtimeGenesisConfigPatch",
            "/genesis/runtime/runtime_genesis_config",
            "/genesis/runtime"
        ];

        for pointer in pointers {
            if chain_spec_json.pointer(pointer).is_some() {
                return Ok(pointer.to_string());
            }
        }

        Err(())
    }
    // Override `genesis` key if present
    fn override_genesis() {}
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
    fn add_balances(runtime_config_ptr: &str, chain_spec_json: &mut serde_json::Value, nodes: &Vec<NodeSpec>, staking_min: u128) {
        if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
            let Some(balances) = val.pointer("/balances/balances") else {
                // should be a info log
                println!("NO 'balances' key in runtime config, skipping...");
                return;
            };

            // create a balance map
            // SAFETY: balances is always an array in chain-spec with items [k,v]
            let mut balances_map: HashMap<String, u128> = serde_json::from_value::<Vec<(String, u128)>>(balances.clone()).unwrap().iter().fold(HashMap::new(), |mut memo, balance| {
                memo.insert(balance.0.clone(), balance.1);
                memo
            });

            for node in nodes {
                if node.initial_balance.eq(&0) { continue };

                // TODO: handle error here and check the `accounts.accounts` design
                let account = node.accounts.accounts.get("sr").unwrap();
                balances_map.insert(account.address.clone(), std::cmp::max(node.initial_balance, staking_min));
            }

            // convert the map and store again
            let new_balances: Vec<(&String, &u128)> = balances_map.iter().collect::<Vec<(&String, &u128)>>();

            val["balances"]["balances"] = json!(new_balances);
        } else {
            unreachable!("pointer to runtime config should be valid!")
        }
    }

    fn get_node_keys(node: &NodeSpec) -> GenesisNodeKey  {
        let sr_account = node.accounts.accounts.get("sr").unwrap();
        let ed_account = node.accounts.accounts.get("ed").unwrap();
        let _ec_account = node.accounts.accounts.get("ec").unwrap();
        let mut keys = HashMap::new();
        for k in ["babe", "im_online", "parachain_validator", "authority_discovery", "para_validator", "para_assignment", "aura", "nimbus", "vrf"] {
            keys.insert(k.to_string(), sr_account.address.clone());
        }

        keys.insert("grandpa".to_string(), ed_account.address.clone());
        // TODO: verify beefy key
        //beefy: encodeAddress(ec_account.publicKey),

        (
            sr_account.address.clone(),
            sr_account.address.clone(),
            keys
        )
    }
    fn add_authorities(runtime_config_ptr: &str, chain_spec_json: &mut serde_json::Value, nodes: &Vec<&NodeSpec>, _key_type: KeyType) {
        if let Some(val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
            let keys: Vec<GenesisNodeKey> = nodes.iter().map(|node| get_node_keys(node)).collect();
            println!("{:#?}", keys);
            val["session"]["keys"] = json!(keys);
        } else {
            unreachable!("pointer to runtime config should be valid!")
        }
    }
    fn add_hrmp_channels(runtime_config_ptr: &str, chain_spec_json: &mut serde_json::Value, _hrmp_channels: &Vec<HrmpChannelConfig>) {
        if let Some(_val) = chain_spec_json.pointer_mut(runtime_config_ptr) {
            todo!()
        } else {
            unreachable!("pointer to runtime config should be valid!")
        }
    }

    // TODO: (team)
    // fn add_aura_authorities() {}
    // fn add_grandpa_authorities() {}
    // fn add_staking() {}
    // fn add_nominators() {}


    // // TODO: (team) we should think a better way to use the decorators from
    // // current version (ts).
    // fn para_custom() { todo!() }
    // fn add_collator_selection() {}

#[cfg(test)]
mod tests {}
