// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code, clippy::expect_fun_call)]

pub mod errors;
pub mod generators;
pub mod network;
pub mod network_helper;
mod network_spec;
#[cfg(feature = "pjs")]
pub mod pjs_helper;
pub mod shared;
mod spawner;

use std::{
    collections::HashSet,
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use configuration::{NetworkConfig, RegistrationStrategy};
use errors::OrchestratorError;
use generators::errors::GeneratorError;
use network::{node::NetworkNode, parachain::Parachain, relaychain::Relaychain, Network};
// re-exported
pub use network_spec::NetworkSpec;
use network_spec::{node::NodeSpec, parachain::ParachainSpec};
use provider::{
    types::{ProviderCapabilities, TransferedFile},
    DynProvider,
};
use support::fs::{FileSystem, FileSystemError};
use tokio::time::timeout;
use tracing::{debug, info, trace};

use crate::{
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

    pub async fn spawn_from_spec(
        &self,
        network_spec: NetworkSpec,
    ) -> Result<Network<T>, OrchestratorError> {
        let global_timeout = network_spec.global_settings.network_spawn_timeout();
        let res = timeout(
            Duration::from_secs(global_timeout as u64),
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
        let ns = if let Some(base_dir) = network_spec.global_settings.base_dir() {
            self.provider
                .create_namespace_with_base_dir(base_dir)
                .await?
        } else {
            self.provider.create_namespace().await?
        };

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

        let relay_chain_name = network_spec.relaychain.chain.as_str().to_owned();
        let base_dir_exists = network_spec.global_settings.base_dir().is_some();
        network_spec
            .build_parachain_artifacts(ns.clone(), &scoped_fs, &relay_chain_id, base_dir_exists)
            .await?;

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
            let genesis_config = para.get_genesis_config()?;
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
        network_spec
            .relaychain
            .chain_spec
            .build_raw(&ns, &scoped_fs)
            .await?;

        let (bootnodes, relaynodes) = split_nodes_by_bootnodes(&network_spec.relaychain.nodes);

        // TODO: we want to still supporting spawn a dedicated bootnode??
        let mut ctx = SpawnNodeCtx {
            chain_id: &relay_chain_id,
            parachain_id: None,
            chain: relay_chain_name.as_str(),
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
            .iter()
            .map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

        // Initiate the node_ws_uel which will be later used in the Parachain_with_extrinsic config
        let mut node_ws_url: String = "".to_string();

        // Calculate the bootnodes addr from the running nodes
        let mut bootnodes_addr: Vec<String> = vec![];
        for node in futures::future::try_join_all(spawning_tasks).await? {
            let ip = node.inner.ip().await?;
            let port = if ctx.ns.capabilities().use_default_ports_in_cmd {
                P2P_PORT
            } else {
                node.spec.p2p_port.0
            };
            let bootnode_multiaddr = generate_bootnode_addr(&node, &ip, port)?;
            bootnodes_addr.push(bootnode_multiaddr);

            // Is used in the register_para_options (We need to get this from the relay and not the collators)
            if node_ws_url.is_empty() {
                node_ws_url.clone_from(&node.ws_uri)
            }

            // Add the node to the `Network` instance
            network.add_running_node(node, None);
        }

        // Add the bootnodes to the relaychain spec file and ctx
        network_spec
            .relaychain
            .chain_spec
            .add_bootnodes(&scoped_fs, &bootnodes_addr)
            .await?;

        ctx.bootnodes_addr = &bootnodes_addr;

        // spawn the rest of the nodes (TODO: in batches)
        let spawning_tasks = relaynodes
            .iter()
            .map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

        for node in futures::future::try_join_all(spawning_tasks).await? {
            // Add the node to the `Network` instance
            network.add_running_node(node, None);
        }

        // spawn paras
        for para in network_spec.parachains.iter() {
            // Create parachain (in the context of the running network)
            let parachain = Parachain::from_spec(para, &global_files_to_inject, &scoped_fs).await?;
            let parachain_id = parachain.chain_id.clone();

            let (bootnodes, collators) = split_nodes_by_bootnodes(&para.collators);

            // Create `ctx` for spawn parachain nodes
            let mut ctx_para = SpawnNodeCtx {
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

            let spawning_tasks = bootnodes.iter().map(|node| {
                spawner::spawn_node(node, parachain.files_to_inject.clone(), &ctx_para)
            });

            // Calculate the bootnodes addr from the running nodes
            let mut bootnodes_addr: Vec<String> = vec![];
            let mut running_nodes: Vec<NetworkNode> = vec![];
            for node in futures::future::try_join_all(spawning_tasks).await? {
                let ip = node.inner.ip().await?;
                let port = if ctx.ns.capabilities().use_default_ports_in_cmd {
                    P2P_PORT
                } else {
                    node.spec.p2p_port.0
                };
                let bootnode_multiaddr = generate_bootnode_addr(&node, &ip, port)?;
                bootnodes_addr.push(bootnode_multiaddr);

                running_nodes.push(node);
            }

            if let Some(para_chain_spec) = para.chain_spec.as_ref() {
                para_chain_spec
                    .add_bootnodes(&scoped_fs, &bootnodes_addr)
                    .await?;
            }

            ctx_para.bootnodes_addr = &bootnodes_addr;

            // Spawn the rest of the nodes
            let spawning_tasks = collators.iter().map(|node| {
                spawner::spawn_node(node, parachain.files_to_inject.clone(), &ctx_para)
            });

            // join all the running nodes
            running_nodes.extend_from_slice(
                futures::future::try_join_all(spawning_tasks)
                    .await?
                    .as_slice(),
            );

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

        // - write zombie.json state file
        let mut zombie_json = serde_json::to_value(&network)?;
        zombie_json["local_base_dir"] = serde_json::value::Value::String(base_dir.to_string());

        scoped_fs
            .write("zombie.json", serde_json::to_string_pretty(&zombie_json)?)
            .await?;
        Ok(network)
    }
}

// Helpers

// Split the node list depending if it's bootnode or not
// NOTE: if there isn't a bootnode declared we use the first one
fn split_nodes_by_bootnodes(nodes: &[NodeSpec]) -> (Vec<&NodeSpec>, Vec<&NodeSpec>) {
    // get the bootnodes to spawn first and calculate the bootnode string for use later
    let mut bootnodes = vec![];
    let mut other_nodes = vec![];
    nodes.iter().for_each(|node| {
        if node.is_bootnode {
            bootnodes.push(node)
        } else {
            other_nodes.push(node)
        }
    });

    if bootnodes.is_empty() {
        bootnodes.push(other_nodes.remove(0))
    }
    (bootnodes, other_nodes)
}

// Generate a bootnode multiaddress and return as string
fn generate_bootnode_addr(
    node: &NetworkNode,
    ip: &IpAddr,
    port: u16,
) -> Result<String, GeneratorError> {
    generators::generate_node_bootnode_addr(
        &node.spec.peer_id,
        ip,
        port,
        node.inner.args().as_ref(),
        &node.spec.p2p_cert_hash,
    )
}
// Validate that the config fulfill all the requirements of the provider
fn validate_spec_with_provider_capabilities(
    network_spec: &NetworkSpec,
    capabilities: &ProviderCapabilities,
) -> Result<(), anyhow::Error> {
    let mut errs: Vec<String> = vec![];

    if capabilities.requires_image {
        // Relaychain
        if network_spec.relaychain.default_image.is_none() {
            // we should check if each node have an image
            let nodes = &network_spec.relaychain.nodes;
            if nodes.iter().any(|node| node.image.is_none()) {
                errs.push(String::from(
                    "Missing image for node, and not default is set at relaychain",
                ));
            }
        };

        // Paras
        for para in &network_spec.parachains {
            if para.default_image.is_none() {
                let nodes = &para.collators;
                if nodes.iter().any(|node| node.image.is_none()) {
                    errs.push(format!(
                        "Missing image for node, and not default is set at parachain {}",
                        para.id
                    ));
                }
            }
        }
    } else {
        // native
        // We need to get all the `cmds` and verify if are part of the path
        let mut cmds: HashSet<&str> = Default::default();
        if let Some(cmd) = network_spec.relaychain.default_command.as_ref() {
            cmds.insert(cmd.as_str());
        }
        for node in network_spec.relaychain().nodes.iter() {
            cmds.insert(node.command());
        }

        // Paras
        for para in &network_spec.parachains {
            if let Some(cmd) = para.default_command.as_ref() {
                cmds.insert(cmd.as_str());
            }

            for node in para.collators.iter() {
                cmds.insert(node.command());
            }
        }

        // now check the binaries
        let path = std::env::var("PATH").unwrap_or_default(); // path should always be set
        trace!("current PATH: {path}");
        let parts: Vec<_> = path.split(":").collect();
        for cmd in cmds {
            let missing = if cmd.contains('/') {
                trace!("checking {cmd}");
                std::fs::metadata(cmd).is_err()
            } else {
                // should be in the PATH
                !parts.iter().any(|part| {
                    let path_to = format!("{}/{}", part, cmd);
                    trace!("checking {path_to}");
                    std::fs::metadata(path_to).is_ok()
                })
            };

            if missing {
                errs.push(help_msg(cmd));
            }
        }
    }

    if !errs.is_empty() {
        let msg = errs.join("\n");
        return Err(anyhow::anyhow!(format!("Invalid configuration: \n {msg}")));
    }

    Ok(())
}

fn help_msg(cmd: &str) -> String {
    match cmd {
        "parachain-template-node" | "solochain-template-node" | "minimal-template-node" => {
            format!("Missing binary {cmd}, compile by running: \n\tcargo build --package {cmd} --release")
        },
        "polkadot" => {
            format!("Missing binary {cmd}, compile by running (in the polkadot-sdk repo): \n\t cargo build --locked --release --features fast-runtime --bin {cmd} --bin polkadot-prepare-worker --bin polkadot-execute-worker")
        },
        "polkadot-parachain" => {
            format!("Missing binary {cmd}, compile by running (in the polkadot-sdk repo): \n\t cargo build --release --locked -p {cmd}-bin --bin {cmd}")
        },
        _ => {
            format!("Missing binary {cmd}, please compile it.")
        },
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
    pub fn new(fs: &'a FS, base_dir: &'a str) -> Self {
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
        let file = file.as_ref();

        let full_path = if file.is_absolute() {
            file.to_owned()
        } else {
            PathBuf::from(format!("{}/{}", self.base_dir, file.to_string_lossy()))
        };
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
        let path = path.as_ref();

        let full_path = if path.is_absolute() {
            path.to_owned()
        } else {
            PathBuf::from(format!("{}/{}", self.base_dir, path.to_string_lossy()))
        };

        self.fs.write(full_path, contents).await.map_err(Into::into)
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

// re-exports
pub use network::{AddCollatorOptions, AddNodeOptions};
pub use network_helper::metrics;
#[cfg(feature = "pjs")]
pub use pjs_helper::PjsResult;

#[cfg(test)]
mod tests {
    use configuration::NetworkConfigBuilder;

    use super::*;

    fn generate(
        with_image: bool,
        with_cmd: Option<&'static str>,
    ) -> Result<NetworkConfig, Vec<anyhow::Error>> {
        NetworkConfigBuilder::new()
            .with_relaychain(|r| {
                let mut relay = r
                    .with_chain("rococo-local")
                    .with_default_command(with_cmd.unwrap_or("polkadot"));
                if with_image {
                    relay = relay.with_default_image("docker.io/parity/polkadot")
                }

                relay
                    .with_node(|node| node.with_name("alice"))
                    .with_node(|node| node.with_name("bob"))
            })
            .with_parachain(|p| {
                p.with_id(2000).cumulus_based(true).with_collator(|n| {
                    let node = n
                        .with_name("collator")
                        .with_command(with_cmd.unwrap_or("polkadot-parachain"));
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
        let network_config = generate(true, None).unwrap();
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
    async fn invalid_config_without_image() {
        let network_config = generate(false, None).unwrap();
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

    #[tokio::test]
    async fn invalid_config_missing_cmd() {
        let network_config = generate(false, Some("other")).unwrap();
        let spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let caps = ProviderCapabilities {
            requires_image: false,
            has_resources: false,
            prefix_with_full_path: false,
            use_default_ports_in_cmd: false,
        };

        let valid = validate_spec_with_provider_capabilities(&spec, &caps);
        assert!(valid.is_err())
    }

    #[tokio::test]
    async fn valid_config_present_cmd() {
        let network_config = generate(false, Some("cargo")).unwrap();
        let spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let caps = ProviderCapabilities {
            requires_image: false,
            has_resources: false,
            prefix_with_full_path: false,
            use_default_ports_in_cmd: false,
        };

        let valid = validate_spec_with_provider_capabilities(&spec, &caps);
        println!("{:?}", valid);
        assert!(valid.is_ok())
    }
}
