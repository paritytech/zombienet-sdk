// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code, clippy::expect_fun_call)]

pub mod errors;
mod generators;
pub mod network;
mod network_helper;
mod network_spec;
mod shared;
mod spawner;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use configuration::{NetworkConfig, RegistrationStrategy};
use errors::OrchestratorError;
use network::{parachain::Parachain, relaychain::Relaychain, Network};
use network_spec::{parachain::ParachainSpec, NetworkSpec};
use provider::{
    types::{ProviderCapabilities, TransferedFile},
    DynProvider,
};
use support::fs::{FileSystem, FileSystemError};
use tokio::time::timeout;
use tracing::{debug, info, trace};

use crate::{
    generators::chain_spec::ParaGenesisConfig,
    shared::{constants::P2P_PORT, types::RegisterParachainOptions},
    spawner::SpawnNodeCtx,
};
pub struct Orchestrator<T>
where
    T: FileSystem + Sync + Send,
{
    filesystem: T,
    provider: DynProvider,
}

impl<T> Orchestrator<T>
where
    T: FileSystem + Sync + Send + Clone,
{
    pub fn new(filesystem: T, provider: DynProvider) -> Self {
        Self {
            filesystem,
            provider,
        }
    }

    pub async fn spawn(
        &self,
        network_config: NetworkConfig,
    ) -> Result<Network<T>, OrchestratorError> {
        let global_timeout = network_config.global_settings().network_spawn_timeout();
        let network_spec = NetworkSpec::from_config(&network_config).await?;

        let res = timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_spec),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout));
        res?
    }

    async fn spawn_inner(
        &self,
        mut network_spec: NetworkSpec,
    ) -> Result<Network<T>, OrchestratorError> {
        // main driver for spawn the network
        debug!(network_spec = ?network_spec,"Network spec to spawn");

        // TODO: move to Provider trait
        validate_spec_with_provider_capabilities(&network_spec, self.provider.capabilities())
            .map_err(|err| {
                OrchestratorError::InvalidConfigForProvider(
                    self.provider.name().into(),
                    err.to_string(),
                )
            })?;

        // create namespace
        let ns = self.provider.create_namespace().await?;
        info!("🧰 ns: {}", ns.name());
        info!("🧰 base_dir: {:?}", ns.base_dir());

        network_spec
            .populate_nodes_available_args(ns.clone())
            .await?;

        let base_dir = ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);
        // Create chain-spec for relaychain
        network_spec
            .relaychain
            .chain_spec
            .build(&ns, &scoped_fs)
            .await?;

        debug!("relaychain spec built!");
        // Create parachain artifacts (chain-spec, wasm, state)
        let relay_chain_id = network_spec
            .relaychain
            .chain_spec
            .read_chain_id(&scoped_fs)
            .await?;
        let relay_chain_name = network_spec.relaychain.chain.as_str();
        // TODO: if we don't need to register this para we can skip it
        for para in network_spec.parachains.iter_mut() {
            let chain_spec_raw_path = para
                .build_chain_spec(&relay_chain_id, &ns, &scoped_fs)
                .await?;
            debug!("parachain chain-spec built!");

            // TODO: this need to be abstracted in a single call to generate_files.
            scoped_fs.create_dir(para.id.to_string()).await?;
            // create wasm/state
            para.genesis_state
                .build(
                    chain_spec_raw_path.clone(),
                    format!("{}/genesis-state", para.id),
                    &ns,
                    &scoped_fs,
                )
                .await?;
            debug!("parachain genesis state built!");
            para.genesis_wasm
                .build(
                    chain_spec_raw_path,
                    format!("{}/genesis-wasm", para.id),
                    &ns,
                    &scoped_fs,
                )
                .await?;
            debug!("parachain genesis wasm built!");
        }

        // Gather the parachains to register in genesis and the ones to register with extrinsic
        let (para_to_register_in_genesis, para_to_register_with_extrinsic): (
            Vec<&ParachainSpec>,
            Vec<&ParachainSpec>,
        ) = network_spec
            .parachains
            .iter()
            .filter(|para| para.registration_strategy != RegistrationStrategy::Manual)
            .partition(|para| {
                matches!(para.registration_strategy, RegistrationStrategy::InGenesis)
            });

        let mut para_artifacts = vec![];
        for para in para_to_register_in_genesis {
            let genesis_config = ParaGenesisConfig {
                state_path: para.genesis_state.artifact_path().ok_or(
                    OrchestratorError::InvariantError(
                        "artifact path for state must be set at this point",
                    ),
                )?,
                wasm_path: para.genesis_wasm.artifact_path().ok_or(
                    OrchestratorError::InvariantError(
                        "artifact path for wasm must be set at this point",
                    ),
                )?,
                id: para.id,
                as_parachain: para.onboard_as_parachain,
            };
            para_artifacts.push(genesis_config)
        }

        // Customize relaychain
        network_spec
            .relaychain
            .chain_spec
            .customize_relay(
                &network_spec.relaychain,
                &network_spec.hrmp_channels,
                para_artifacts,
                &scoped_fs,
            )
            .await?;

        // Build raw version
        network_spec.relaychain.chain_spec.build_raw(&ns).await?;

        // get the bootnodes to spawn first and calculate the bootnode string for use later
        let mut bootnodes = vec![];
        let mut relaynodes = vec![];
        network_spec.relaychain.nodes.iter().for_each(|node| {
            if node.is_bootnode {
                bootnodes.push(node)
            } else {
                relaynodes.push(node)
            }
        });

        if bootnodes.is_empty() {
            bootnodes.push(relaynodes.remove(0))
        }

        // TODO: we want to still supporting spawn a dedicated bootnode??
        let mut ctx = SpawnNodeCtx {
            chain_id: &relay_chain_id,
            parachain_id: None,
            chain: relay_chain_name,
            role: ZombieRole::Node,
            ns: &ns,
            scoped_fs: &scoped_fs,
            parachain: None,
            bootnodes_addr: &vec![],
            wait_ready: false,
        };

        let global_files_to_inject = vec![TransferedFile::new(
            PathBuf::from(format!(
                "{}/{relay_chain_name}.json",
                ns.base_dir().to_string_lossy()
            )),
            PathBuf::from(format!("/cfg/{relay_chain_name}.json")),
        )];

        let r = Relaychain::new(
            relay_chain_name.to_string(),
            relay_chain_id.clone(),
            PathBuf::from(network_spec.relaychain.chain_spec.raw_path().ok_or(
                OrchestratorError::InvariantError("chain-spec raw path should be set now"),
            )?),
        );
        let mut network =
            Network::new_with_relay(r, ns.clone(), self.filesystem.clone(), network_spec.clone());

        let spawning_tasks = bootnodes
            .iter_mut()
            .map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

        // Initiate the node_ws_uel which will be later used in the Parachain_with_extrinsic config
        let mut node_ws_url: String = "".to_string();

        // Calculate the bootnodes addr from the running nodes
        let mut bootnodes_addr: Vec<String> = vec![];
        for node in futures::future::try_join_all(spawning_tasks).await? {
            let ip = node.inner.ip().await?;
            bootnodes_addr.push(
                // TODO: we just use localhost for now
                generators::generate_node_bootnode_addr(
                    &node.spec.peer_id,
                    &ip,
                    if ctx.ns.capabilities().use_default_ports_in_cmd {
                        P2P_PORT
                    } else {
                        node.spec.p2p_port.0
                    },
                    node.inner.args().as_ref(),
                    &node.spec.p2p_cert_hash,
                )?,
            );

            // Is used in the register_para_options (We need to get this from the relay and not the collators)
            if node_ws_url.is_empty() {
                node_ws_url = node.ws_uri.clone()
            }

            // Add the node to the `Network` instance
            network.add_running_node(node, None);
        }

        ctx.bootnodes_addr = &bootnodes_addr;

        // spawn the rest of the nodes (TODO: in batches)
        let spawning_tasks = relaynodes
            .iter()
            .map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

        for node in futures::future::try_join_all(spawning_tasks).await? {
            // Add the node to the `Network` instance
            network.add_running_node(node, None);
        }

        // Add the bootnodes to the relaychain spec file
        network_spec
            .relaychain
            .chain_spec
            .add_bootnodes(&scoped_fs, &bootnodes_addr)
            .await?;

        // spawn paras
        for para in network_spec.parachains.iter() {
            // Create parachain (in the context of the running network)
            let parachain = Parachain::from_spec(para, &global_files_to_inject, &scoped_fs).await?;
            let parachain_id = parachain.chain_id.clone();

            // Create `ctx` for spawn the nodes
            let ctx_para = SpawnNodeCtx {
                parachain: Some(para),
                parachain_id: parachain_id.as_deref(),
                role: if para.is_cumulus_based {
                    ZombieRole::CumulusCollator
                } else {
                    ZombieRole::Collator
                },
                bootnodes_addr: &vec![],
                ..ctx.clone()
            };

            let mut para_files_to_inject = global_files_to_inject.clone();
            if para.is_cumulus_based {
                para_files_to_inject.push(TransferedFile::new(
                    PathBuf::from(format!(
                        "{}/{}.json",
                        ns.base_dir().to_string_lossy(),
                        para.id
                    )),
                    PathBuf::from(format!("/cfg/{}.json", para.id)),
                ));
            }

            // Spawn the nodes
            let spawning_tasks = para.collators.iter().map(|node| {
                spawner::spawn_node(node, parachain.files_to_inject.clone(), &ctx_para)
            });

            let running_nodes = futures::future::try_join_all(spawning_tasks).await?;
            let running_para_id = parachain.para_id;
            network.add_para(parachain);
            for node in running_nodes {
                network.add_running_node(node, Some(running_para_id));
            }
        }

        // TODO:
        // - add-ons (introspector/tracing/etc)

        // verify nodes
        // network_helper::verifier::verify_nodes(&network.nodes()).await?;

        // Now we need to register the paras with extrinsic from the Vec collected before;
        for para in para_to_register_with_extrinsic {
            let register_para_options: RegisterParachainOptions = RegisterParachainOptions {
                id: para.id,
                // This needs to resolve correctly
                wasm_path: para
                    .genesis_wasm
                    .artifact_path()
                    .ok_or(OrchestratorError::InvariantError(
                        "artifact path for wasm must be set at this point",
                    ))?
                    .to_path_buf(),
                state_path: para
                    .genesis_state
                    .artifact_path()
                    .ok_or(OrchestratorError::InvariantError(
                        "artifact path for state must be set at this point",
                    ))?
                    .to_path_buf(),
                node_ws_url: node_ws_url.clone(),
                onboard_as_para: para.onboard_as_parachain,
                seed: None, // TODO: Seed is passed by?
                finalization: false,
            };

            Parachain::register(register_para_options, &scoped_fs).await?;
        }

        // - write zombie.json state file (we should defined in a way we can load later)

        Ok(network)
    }
}

// Validate that the config fulfill all the requirements of the provider
fn validate_spec_with_provider_capabilities(
    network_spec: &NetworkSpec,
    capabilities: &ProviderCapabilities,
) -> Result<(), anyhow::Error> {
    if !capabilities.requires_image {
        return Ok(());
    }

    // Relaychain
    if network_spec.relaychain.default_image.is_none() {
        // we should check if each node have an image
        let nodes = &network_spec.relaychain.nodes;
        if nodes.iter().any(|node| node.image.is_none()) {
            return Err(anyhow::anyhow!(
                "missing image for node, and not default is set at relaychain"
            ));
        }
    };

    // Paras
    for para in &network_spec.parachains {
        if para.default_image.is_none() {
            let nodes = &para.collators;
            if nodes.iter().any(|node| node.image.is_none()) {
                return Err(anyhow::anyhow!(
                    "missing image for node, and not default is set at parachain {}",
                    para.id
                ));
            }
        }
    }

    Ok(())
}
// TODO: get the fs from `DynNamespace` will make this not needed
// but the FileSystem trait isn't object-safe so we can't pass around
// as `dyn FileSystem`. We can refactor or using some `erase` techniques
// to resolve this and remove this struct
// TODO (Loris): Probably we could have a .scoped(base_dir) method on the
// filesystem itself (the trait), so it will return this and we can move this
// directly to the support crate, it can be useful in the future
#[derive(Clone, Debug)]
pub struct ScopedFilesystem<'a, FS: FileSystem> {
    fs: &'a FS,
    base_dir: &'a str,
}

impl<'a, FS: FileSystem> ScopedFilesystem<'a, FS> {
    fn new(fs: &'a FS, base_dir: &'a str) -> Self {
        Self { fs, base_dir }
    }

    async fn copy_files(&self, files: Vec<&TransferedFile>) -> Result<(), FileSystemError> {
        for file in files {
            let full_remote_path = PathBuf::from(format!(
                "{}/{}",
                self.base_dir,
                file.remote_path.to_string_lossy()
            ));
            trace!("coping file: {file}");
            self.fs
                .copy(file.local_path.as_path(), full_remote_path)
                .await?;
        }
        Ok(())
    }

    async fn read_to_string(&self, file: impl AsRef<Path>) -> Result<String, FileSystemError> {
        let full_path = PathBuf::from(format!(
            "{}/{}",
            self.base_dir,
            file.as_ref().to_string_lossy()
        ));
        let content = self.fs.read_to_string(full_path).await?;
        Ok(content)
    }

    async fn create_dir(&self, path: impl AsRef<Path>) -> Result<(), FileSystemError> {
        let path = PathBuf::from(format!(
            "{}/{}",
            self.base_dir,
            path.as_ref().to_string_lossy()
        ));
        self.fs.create_dir(path).await.map_err(Into::into)
    }

    async fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), FileSystemError> {
        let path = PathBuf::from(format!(
            "{}/{}",
            self.base_dir,
            path.as_ref().to_string_lossy()
        ));
        self.fs.create_dir_all(path).await.map_err(Into::into)
    }

    async fn write(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]> + Send,
    ) -> Result<(), FileSystemError> {
        let path = PathBuf::from(format!(
            "{}/{}",
            self.base_dir,
            path.as_ref().to_string_lossy()
        ));
        self.fs.write(path, contents).await.map_err(Into::into)
    }
}

#[derive(Clone, Debug)]
pub enum ZombieRole {
    Temp,
    Node,
    Bootnode,
    Collator,
    CumulusCollator,
    Companion,
}

// re-export
pub use network::{AddCollatorOptions, AddNodeOptions};
pub use shared::types::PjsResult;

#[cfg(test)]
mod tests {
    use configuration::NetworkConfigBuilder;

    use super::*;

    fn generate(with_image: bool) -> Result<NetworkConfig, Vec<anyhow::Error>> {
        NetworkConfigBuilder::new()
            .with_relaychain(|r| {
                let mut relay = r
                    .with_chain("rococo-local")
                    .with_default_command("polkadot");
                if with_image {
                    relay = relay.with_default_image("docker.io/parity/polkadot")
                }

                relay
                    .with_node(|node| node.with_name("alice"))
                    .with_node(|node| node.with_name("bob"))
            })
            .with_parachain(|p| {
                p.with_id(2000).cumulus_based(true).with_collator(|n| {
                    let node = n.with_name("collator").with_command("polkadot-parachain");
                    if with_image {
                        node.with_image("docker.io/paritypr/test-parachain")
                    } else {
                        node
                    }
                })
            })
            .build()
    }

    #[tokio::test]
    async fn valid_config_with_image() {
        let network_config = generate(true).unwrap();
        let spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let caps = ProviderCapabilities {
            requires_image: true,
            has_resources: false,
            prefix_with_full_path: false,
            use_default_ports_in_cmd: false,
        };

        let valid = validate_spec_with_provider_capabilities(&spec, &caps);
        assert!(valid.is_ok())
    }

    #[tokio::test]
    async fn invalid_config() {
        let network_config = generate(false).unwrap();
        let spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let caps = ProviderCapabilities {
            requires_image: true,
            has_resources: false,
            prefix_with_full_path: false,
            use_default_ports_in_cmd: false,
        };

        let valid = validate_spec_with_provider_capabilities(&spec, &caps);
        assert!(valid.is_err())
    }
}
