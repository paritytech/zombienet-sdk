// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code)]

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
use provider::{constants::LOCALHOST, types::TransferedFile, Provider};
use support::fs::{FileSystem, FileSystemError};
use tokio::time::timeout;
use tracing::{debug, info};

use crate::{
    generators::chain_spec::ParaGenesisConfig, shared::types::RegisterParachainOptions,
    spawner::SpawnNodeCtx,
};
pub struct Orchestrator<T, P>
where
    T: FileSystem + Sync + Send,
    P: Provider,
{
    filesystem: T,
    provider: P,
}

impl<T, P> Orchestrator<T, P>
where
    T: FileSystem + Sync + Send + Clone,
    P: Provider,
{
    pub fn new(filesystem: T, provider: P) -> Self {
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

        timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_spec),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout))?
    }

    async fn spawn_inner(
        &self,
        mut network_spec: NetworkSpec,
    ) -> Result<Network<T>, OrchestratorError> {
        // main driver for spawn the network
        debug!("Network spec to spawn, {:#?}", network_spec);

        // create namespace
        let ns = self.provider.create_namespace().await?;

        info!("🧰 ns: {}", ns.name());
        info!("🧰 base_dir: {:?}", ns.base_dir());

        // TODO: noop for native
        // Static setup
        // ns.static_setup().await?;

        let base_dir = ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);
        // Create chain-spec for relaychain
        network_spec
            .relaychain
            .chain_spec
            .build(&ns, &scoped_fs)
            .await?;

        // Create parachain artifacts (chain-spec, wasm, state)
        let relay_chain_id = network_spec
            .relaychain
            .chain_spec
            .read_chain_id(&scoped_fs)
            .await?;
        let relay_chain_name = network_spec.relaychain.chain.as_str();
        // TODO: if we don't need to register this para we can skip it
        for para in network_spec.parachains.iter_mut() {
            let para_cloned = para.clone();
            let chain_spec_raw_path = if let Some(chain_spec) = para.chain_spec.as_mut() {
                chain_spec.build(&ns, &scoped_fs).await?;
                debug!("chain_spec: {:#?}", chain_spec);

                chain_spec
                    .customize_para(&para_cloned, &relay_chain_id, &scoped_fs)
                    .await?;
                chain_spec.build_raw(&ns).await?;

                let chain_spec_raw_path =
                    chain_spec
                        .raw_path()
                        .ok_or(OrchestratorError::InvariantError(
                            "chain-spec raw path should be set now",
                        ))?;
                Some(chain_spec_raw_path)
            } else {
                None
            };

            // TODO: this need to be abstracted in a single call to generate_files.
            scoped_fs.create_dir(para.id.to_string()).await?;
            // create wasm/state
            para.genesis_state
                .build(
                    chain_spec_raw_path,
                    format!("{}/genesis-state", para.id),
                    &ns,
                    &scoped_fs,
                )
                .await?;
            para.genesis_wasm
                .build(
                    chain_spec_raw_path,
                    format!("{}/genesis-wasm", para.id),
                    &ns,
                    &scoped_fs,
                )
                .await?;
        }

        // Gather the parachains to register in genesis and the ones to register with extrinsic
        let (para_to_register_in_genesis, para_to_register_with_extrinsic): (
            Vec<&ParachainSpec>,
            Vec<&ParachainSpec>,
        ) = network_spec.parachains.iter().partition(|para| {
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

        let global_files_to_inject = vec![TransferedFile {
            local_path: PathBuf::from(format!(
                "{}/{relay_chain_name}.json",
                ns.base_dir().to_string_lossy()
            )),
            remote_path: PathBuf::from(format!("/cfg/{relay_chain_name}.json")),
        }];

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
            bootnodes_addr.push(
                // TODO: we just use localhost for now
                generators::generate_node_bootnode_addr(
                    &node.spec.peer_id,
                    &LOCALHOST,
                    node.spec.p2p_port.0,
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
            // global files to include for this parachain
            let mut para_files_to_inject = global_files_to_inject.clone();

            // parachain id is used for the keystore
            let parachain_id = if let Some(chain_spec) = para.chain_spec.as_ref() {
                let id = chain_spec.read_chain_id(&scoped_fs).await?;

                // add the spec to global files to inject
                let spec_name = chain_spec.chain_spec_name();
                para_files_to_inject.push(TransferedFile {
                    local_path: ns.base_dir().join(format!("{}.json", spec_name)),
                    remote_path: PathBuf::from(format!("/cfg/{}.json", para.id)),
                });

                let raw_path = chain_spec
                    .raw_path()
                    .ok_or(OrchestratorError::InvariantError(
                        "chain-spec path should be set by now.",
                    ))?;
                let mut running_para = Parachain::with_chain_spec(para.id, &id, raw_path);
                if let Some(chain_name) = chain_spec.chain_name() {
                    running_para.chain = Some(chain_name.to_string());
                }
                network.add_para(running_para);

                Some(id)
            } else {
                network.add_para(Parachain::new(para.id));

                None
            };

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

            let spawning_tasks = para
                .collators
                .iter()
                .map(|node| spawner::spawn_node(node, para_files_to_inject.clone(), &ctx_para));
            // TODO: Add para to Network instance
            for node in futures::future::try_join_all(spawning_tasks).await? {
                network.add_running_node(node, Some(para.id));
            }
        }

        // TODO:
        // - add-ons (introspector/tracing/etc)

        // verify nodes
        network_helper::verifier::verify_nodes(&network.nodes()).await?;

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
                seed: None,          // TODO: Seed is passed by?
                finalization: false, // TODO: Seed is passed by?
            };

            Parachain::register(register_para_options, &scoped_fs).await?;
        }

        // - write zombie.json state file (we should defined in a way we can load later)

        Ok(network)
    }
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
