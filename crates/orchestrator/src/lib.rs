// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code, clippy::expect_fun_call)]

pub mod errors;
pub mod generators;
pub mod network;
pub mod network_helper;
pub mod tx_helper;

mod network_spec;
pub mod shared;
mod spawner;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    env,
    net::IpAddr,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
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
use serde_json::json;
use support::{
    constants::{
        GRAPH_CONTAINS_DEP, GRAPH_CONTAINS_NAME, INDEGREE_CONTAINS_NAME, QUEUE_NOT_EMPTY,
        THIS_IS_A_BUG,
    },
    fs::{FileSystem, FileSystemError},
    replacer::{get_tokens_to_replace, has_tokens},
};
use tokio::time::timeout;
use tracing::{debug, info, trace, warn};

use crate::{shared::types::RegisterParachainOptions, spawner::SpawnNodeCtx};
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

        // set the spawn_concurrency
        let (spawn_concurrency, limited_by_tokens) = calculate_concurrency(&network_spec)?;

        let start_time = SystemTime::now();
        info!("🧰 ns: {}", ns.name());
        info!("🧰 base_dir: {:?}", ns.base_dir());
        info!("🕰 start time: {:?}", start_time);
        info!("⚙️ spawn concurrency: {spawn_concurrency} (limited by tokens: {limited_by_tokens})");

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
            .build_raw(&ns, &scoped_fs, None)
            .await?;

        // override wasm if needed
        if let Some(ref wasm_override) = network_spec.relaychain.wasm_override {
            network_spec
                .relaychain
                .chain_spec
                .override_code(&scoped_fs, wasm_override)
                .await?;
        }

        // override raw spec if needed
        if let Some(ref raw_spec_override) = network_spec.relaychain.raw_spec_override {
            network_spec
                .relaychain
                .chain_spec
                .override_raw_spec(&scoped_fs, raw_spec_override)
                .await?;
        }

        let (bootnodes, relaynodes) =
            split_nodes_by_bootnodes(&network_spec.relaychain.nodes, false);

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
            nodes_by_name: json!({}),
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

        // Initiate the node_ws_url which will be later used in the Parachain_with_extrinsic config
        let mut node_ws_url: String = "".to_string();

        // Calculate the bootnodes addr from the running nodes
        let mut bootnodes_addr: Vec<String> = vec![];

        for level in dependency_levels_among(&bootnodes)? {
            let mut running_nodes_per_level = vec![];
            for chunk in level.chunks(spawn_concurrency) {
                let spawning_tasks = chunk
                    .iter()
                    .map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

                for node in futures::future::try_join_all(spawning_tasks).await? {
                    let bootnode_multiaddr = node.multiaddr();

                    bootnodes_addr.push(bootnode_multiaddr.to_string());

                    // Is used in the register_para_options (We need to get this from the relay and not the collators)
                    if node_ws_url.is_empty() {
                        node_ws_url.clone_from(&node.ws_uri)
                    }

                    running_nodes_per_level.push(node);
                }
            }
            info!(
                "🕰 waiting for level: {:?} to be up...",
                level.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
            );

            // Wait for all nodes in the current level to be up
            let waiting_tasks = running_nodes_per_level.iter().map(|node| {
                node.wait_until_is_up(network_spec.global_settings.network_spawn_timeout())
            });

            let _ = futures::future::try_join_all(waiting_tasks).await?;

            for node in running_nodes_per_level {
                // Add the node to the  context and `Network` instance
                ctx.nodes_by_name[node.name().to_owned()] = serde_json::to_value(&node)?;
                network.add_running_node(node, None).await;
            }
        }

        // Add the bootnodes to the relaychain spec file and ctx
        network_spec
            .relaychain
            .chain_spec
            .add_bootnodes(&scoped_fs, &bootnodes_addr)
            .await?;

        ctx.bootnodes_addr = &bootnodes_addr;

        for level in dependency_levels_among(&relaynodes)? {
            let mut running_nodes_per_level = vec![];
            for chunk in level.chunks(spawn_concurrency) {
                let spawning_tasks = chunk
                    .iter()
                    .map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

                for node in futures::future::try_join_all(spawning_tasks).await? {
                    running_nodes_per_level.push(node);
                }
            }
            info!(
                "🕰 waiting for level: {:?} to be up...",
                level.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
            );

            // Wait for all nodes in the current level to be up
            let waiting_tasks = running_nodes_per_level.iter().map(|node| {
                node.wait_until_is_up(network_spec.global_settings.network_spawn_timeout())
            });

            let _ = futures::future::try_join_all(waiting_tasks).await?;

            for node in running_nodes_per_level {
                ctx.nodes_by_name[node.name().to_owned()] = serde_json::to_value(&node)?;
                network.add_running_node(node, None).await;
            }
        }

        // spawn paras
        for para in network_spec.parachains.iter() {
            // Create parachain (in the context of the running network)
            let parachain = Parachain::from_spec(para, &global_files_to_inject, &scoped_fs).await?;
            let parachain_id = parachain.chain_id.clone();

            let (bootnodes, collators) =
                split_nodes_by_bootnodes(&para.collators, para.no_default_bootnodes);

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

            // Calculate the bootnodes addr from the running nodes
            let mut bootnodes_addr: Vec<String> = vec![];
            let mut running_nodes: Vec<NetworkNode> = vec![];

            for level in dependency_levels_among(&bootnodes)? {
                let mut running_nodes_per_level = vec![];
                for chunk in level.chunks(spawn_concurrency) {
                    let spawning_tasks = chunk.iter().map(|node| {
                        spawner::spawn_node(node, parachain.files_to_inject.clone(), &ctx_para)
                    });

                    for node in futures::future::try_join_all(spawning_tasks).await? {
                        let bootnode_multiaddr = node.multiaddr();

                        bootnodes_addr.push(bootnode_multiaddr.to_string());

                        running_nodes_per_level.push(node);
                    }
                }
                info!(
                    "🕰 waiting for level: {:?} to be up...",
                    level.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
                );

                // Wait for all nodes in the current level to be up
                let waiting_tasks = running_nodes_per_level.iter().map(|node| {
                    node.wait_until_is_up(network_spec.global_settings.network_spawn_timeout())
                });

                let _ = futures::future::try_join_all(waiting_tasks).await?;

                for node in running_nodes_per_level {
                    ctx_para.nodes_by_name[node.name().to_owned()] = serde_json::to_value(&node)?;
                    running_nodes.push(node);
                }
            }

            if let Some(para_chain_spec) = para.chain_spec.as_ref() {
                para_chain_spec
                    .add_bootnodes(&scoped_fs, &bootnodes_addr)
                    .await?;
            }

            ctx_para.bootnodes_addr = &bootnodes_addr;

            // Spawn the rest of the nodes
            for level in dependency_levels_among(&collators)? {
                let mut running_nodes_per_level = vec![];
                for chunk in level.chunks(spawn_concurrency) {
                    let spawning_tasks = chunk.iter().map(|node| {
                        spawner::spawn_node(node, parachain.files_to_inject.clone(), &ctx_para)
                    });

                    for node in futures::future::try_join_all(spawning_tasks).await? {
                        running_nodes_per_level.push(node);
                    }
                }
                info!(
                    "🕰 waiting for level: {:?} to be up...",
                    level.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
                );

                // Wait for all nodes in the current level to be up
                let waiting_tasks = running_nodes_per_level.iter().map(|node| {
                    node.wait_until_is_up(network_spec.global_settings.network_spawn_timeout())
                });

                let _ = futures::future::try_join_all(waiting_tasks).await?;

                for node in running_nodes_per_level {
                    ctx_para.nodes_by_name[node.name().to_owned()] = serde_json::to_value(&node)?;
                    running_nodes.push(node);
                }
            }

            let running_para_id = parachain.para_id;
            network.add_para(parachain);
            for node in running_nodes {
                network.add_running_node(node, Some(running_para_id)).await;
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
        zombie_json["ns"] = serde_json::value::Value::String(ns.name().to_string());

        if let Ok(start_time_ts) = start_time.duration_since(SystemTime::UNIX_EPOCH) {
            zombie_json["start_time_ts"] =
                serde_json::value::Value::String(start_time_ts.as_millis().to_string());
        } else {
            // Just warn, do not propagate the err (this should not happens)
            warn!("⚠️ Error getting start_time timestamp");
        }

        scoped_fs
            .write("zombie.json", serde_json::to_string_pretty(&zombie_json)?)
            .await?;

        if network_spec.global_settings.tear_down_on_failure() {
            network.spawn_watching_task();
        }

        Ok(network)
    }
}

// Helpers

// Split the node list depending if it's bootnode or not
// NOTE: if there isn't a bootnode declared we use the first one
fn split_nodes_by_bootnodes(
    nodes: &[NodeSpec],
    no_default_bootnodes: bool,
) -> (Vec<&NodeSpec>, Vec<&NodeSpec>) {
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

    if bootnodes.is_empty() && !no_default_bootnodes {
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
                if std::fs::metadata(cmd).is_err() {
                    true
                } else {
                    info!("🔎  We will use the full path {cmd} to spawn nodes.");
                    false
                }
            } else {
                // should be in the PATH
                !parts.iter().any(|part| {
                    let path_to = format!("{part}/{cmd}");
                    trace!("checking {path_to}");
                    let check_result = std::fs::metadata(&path_to);
                    trace!("result {:?}", check_result);
                    if check_result.is_ok() {
                        info!("🔎  We will use the cmd: '{cmd}' at path {path_to} to spawn nodes.");
                        true
                    } else {
                        false
                    }
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

/// Allow to set the default concurrency through env var `ZOMBIE_SPAWN_CONCURRENCY`
fn spawn_concurrency_from_env() -> Option<usize> {
    if let Ok(concurrency) = env::var("ZOMBIE_SPAWN_CONCURRENCY") {
        concurrency.parse::<usize>().ok()
    } else {
        None
    }
}

fn calculate_concurrency(spec: &NetworkSpec) -> Result<(usize, bool), anyhow::Error> {
    let desired_spawn_concurrency = match (
        spawn_concurrency_from_env(),
        spec.global_settings.spawn_concurrency(),
    ) {
        (Some(n), _) => Some(n),
        (None, Some(n)) => Some(n),
        _ => None,
    };

    let (spawn_concurrency, limited_by_tokens) =
        if let Some(spawn_concurrency) = desired_spawn_concurrency {
            if spawn_concurrency == 1 {
                (1, false)
            } else if has_tokens(&serde_json::to_string(spec)?) {
                (1, true)
            } else {
                (spawn_concurrency, false)
            }
        } else {
            // not set
            if has_tokens(&serde_json::to_string(spec)?) {
                (1, true)
            } else {
                // use 100 as max concurrency, we can set a max by provider later
                (100, false)
            }
        };

    Ok((spawn_concurrency, limited_by_tokens))
}

/// Build deterministic dependency **levels** among the given nodes.
/// - Only dependencies **between nodes in `nodes`** are considered.
/// - Unknown/out-of-scope references are ignored.
/// - Self-dependencies are ignored.
fn dependency_levels_among<'a>(
    nodes: &'a [&'a NodeSpec],
) -> Result<Vec<Vec<&'a NodeSpec>>, OrchestratorError> {
    let by_name = nodes
        .iter()
        .map(|n| (n.name.as_str(), *n))
        .collect::<HashMap<_, _>>();

    let mut graph = HashMap::with_capacity(nodes.len());
    let mut indegree = HashMap::with_capacity(nodes.len());

    for node in nodes {
        graph.insert(node.name.as_str(), Vec::new());
        indegree.insert(node.name.as_str(), 0);
    }

    // build dependency graph
    for &node in nodes {
        if let Ok(args_json) = serde_json::to_string(&node.args) {
            // collect dependencies
            let unique_deps = get_tokens_to_replace(&args_json)
                .into_iter()
                .filter(|dep| dep != &node.name)
                .filter_map(|dep| by_name.get(dep.as_str()))
                .map(|&dep_node| dep_node.name.as_str())
                .collect::<HashSet<_>>();

            for dep_name in unique_deps {
                graph
                    .get_mut(dep_name)
                    .expect(&format!("{GRAPH_CONTAINS_DEP} {THIS_IS_A_BUG}"))
                    .push(node);
                *indegree
                    .get_mut(node.name.as_str())
                    .expect(&format!("{INDEGREE_CONTAINS_NAME} {THIS_IS_A_BUG}")) += 1;
            }
        }
    }

    // find all nodes with no dependencies
    let mut queue = nodes
        .iter()
        .filter(|n| {
            *indegree
                .get(n.name.as_str())
                .expect(&format!("{INDEGREE_CONTAINS_NAME} {THIS_IS_A_BUG}"))
                == 0
        })
        .copied()
        .collect::<VecDeque<_>>();

    let mut processed_count = 0;
    let mut levels = Vec::new();

    // Kahn's algorithm
    while !queue.is_empty() {
        let level_size = queue.len();
        let mut current_level = Vec::with_capacity(level_size);

        for _ in 0..level_size {
            let n = queue
                .pop_front()
                .expect(&format!("{QUEUE_NOT_EMPTY} {THIS_IS_A_BUG}"));
            current_level.push(n);
            processed_count += 1;

            for &neighbour in graph
                .get(n.name.as_str())
                .expect(&format!("{GRAPH_CONTAINS_NAME} {THIS_IS_A_BUG}"))
            {
                let neighbour_indegree = indegree
                    .get_mut(neighbour.name.as_str())
                    .expect(&format!("{INDEGREE_CONTAINS_NAME} {THIS_IS_A_BUG}"));
                *neighbour_indegree -= 1;

                if *neighbour_indegree == 0 {
                    queue.push_back(neighbour);
                }
            }
        }

        current_level.sort_by_key(|n| &n.name);
        levels.push(current_level);
    }

    // cycles detected, e.g A -> B -> A
    if processed_count != nodes.len() {
        return Err(OrchestratorError::InvalidConfig(
            "Tokens have cyclical dependencies".to_string(),
        ));
    }

    Ok(levels)
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

    async fn read(&self, file: impl AsRef<Path>) -> Result<Vec<u8>, FileSystemError> {
        let file = file.as_ref();

        let full_path = if file.is_absolute() {
            file.to_owned()
        } else {
            PathBuf::from(format!("{}/{}", self.base_dir, file.to_string_lossy()))
        };
        let content = self.fs.read(full_path).await?;
        Ok(content)
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
        self.fs.create_dir(path).await
    }

    async fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), FileSystemError> {
        let path = PathBuf::from(format!(
            "{}/{}",
            self.base_dir,
            path.as_ref().to_string_lossy()
        ));
        self.fs.create_dir_all(path).await
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

        self.fs.write(full_path, contents).await
    }

    /// Get the full_path in the scoped FS
    fn full_path(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();

        let full_path = if path.is_absolute() {
            path.to_owned()
        } else {
            PathBuf::from(format!("{}/{}", self.base_dir, path.to_string_lossy()))
        };

        full_path
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
pub use sc_chain_spec;

#[cfg(test)]
mod tests {
    use configuration::{GlobalSettingsBuilder, NetworkConfigBuilder};
    use lazy_static::lazy_static;
    use tokio::sync::Mutex;

    use super::*;

    const ENV_KEY: &str = "ZOMBIE_SPAWN_CONCURRENCY";
    // mutex for test that use env
    lazy_static! {
        static ref ENV_MUTEX: Mutex<()> = Mutex::new(());
    }

    fn set_env(concurrency: Option<u32>) {
        if let Some(value) = concurrency {
            env::set_var(ENV_KEY, value.to_string());
        } else {
            env::remove_var(ENV_KEY);
        }
    }

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
                    .with_validator(|node| node.with_name("alice"))
                    .with_validator(|node| node.with_name("bob"))
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

    fn get_node_with_dependencies(name: &str, dependencies: Option<Vec<&NodeSpec>>) -> NodeSpec {
        let mut spec = NodeSpec {
            name: name.to_string(),
            ..Default::default()
        };
        if let Some(dependencies) = dependencies {
            for node in dependencies {
                spec.args.push(
                    format!("{{{{ZOMBIE:{}:someField}}}}", node.name)
                        .as_str()
                        .into(),
                );
            }
        }
        spec
    }

    fn verify_levels(actual_levels: Vec<Vec<&NodeSpec>>, expected_levels: Vec<Vec<&str>>) {
        actual_levels
            .iter()
            .zip(expected_levels)
            .for_each(|(actual_level, expected_level)| {
                assert_eq!(actual_level.len(), expected_level.len());
                actual_level
                    .iter()
                    .zip(expected_level.iter())
                    .for_each(|(node, expected_name)| assert_eq!(node.name, *expected_name));
            });
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
        println!("{valid:?}");
        assert!(valid.is_ok())
    }

    #[tokio::test]
    async fn default_spawn_concurrency() {
        let _g = ENV_MUTEX.lock().await;
        set_env(None);
        let network_config = generate(false, Some("cargo")).unwrap();
        let spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let (concurrency, _) = calculate_concurrency(&spec).unwrap();
        assert_eq!(concurrency, 100);
    }

    #[tokio::test]
    async fn set_spawn_concurrency() {
        let _g = ENV_MUTEX.lock().await;
        set_env(None);

        let network_config = generate(false, Some("cargo")).unwrap();
        let mut spec = NetworkSpec::from_config(&network_config).await.unwrap();

        let global_settings = GlobalSettingsBuilder::new()
            .with_spawn_concurrency(4)
            .build()
            .unwrap();

        spec.set_global_settings(global_settings);
        let (concurrency, limited) = calculate_concurrency(&spec).unwrap();
        assert_eq!(concurrency, 4);
        assert!(!limited);
    }

    #[tokio::test]
    async fn set_spawn_concurrency_but_limited() {
        let _g = ENV_MUTEX.lock().await;
        set_env(None);

        let network_config = generate(false, Some("cargo")).unwrap();
        let mut spec = NetworkSpec::from_config(&network_config).await.unwrap();

        let global_settings = GlobalSettingsBuilder::new()
            .with_spawn_concurrency(4)
            .build()
            .unwrap();

        spec.set_global_settings(global_settings);
        let node = spec.relaychain.nodes.first_mut().unwrap();
        node.args
            .push("--bootnodes {{ZOMBIE:bob:multiAddress')}}".into());
        let (concurrency, limited) = calculate_concurrency(&spec).unwrap();
        assert_eq!(concurrency, 1);
        assert!(limited);
    }

    #[tokio::test]
    async fn set_spawn_concurrency_from_env() {
        let _g = ENV_MUTEX.lock().await;
        set_env(Some(10));

        let network_config = generate(false, Some("cargo")).unwrap();
        let spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let (concurrency, limited) = calculate_concurrency(&spec).unwrap();
        assert_eq!(concurrency, 10);
        assert!(!limited);
    }

    #[tokio::test]
    async fn set_spawn_concurrency_from_env_but_limited() {
        let _g = ENV_MUTEX.lock().await;
        set_env(Some(12));

        let network_config = generate(false, Some("cargo")).unwrap();
        let mut spec = NetworkSpec::from_config(&network_config).await.unwrap();
        let node = spec.relaychain.nodes.first_mut().unwrap();
        node.args
            .push("--bootnodes {{ZOMBIE:bob:multiAddress')}}".into());
        let (concurrency, limited) = calculate_concurrency(&spec).unwrap();
        assert_eq!(concurrency, 1);
        assert!(limited);
    }

    #[test]
    fn dependency_levels_among_should_work() {
        // no nodes
        assert!(dependency_levels_among(&[]).unwrap().is_empty());

        // one node
        let alice = get_node_with_dependencies("alice", None);
        let nodes = [&alice];

        let levels = dependency_levels_among(&nodes).unwrap();
        let expected = vec![vec!["alice"]];

        verify_levels(levels, expected);

        // two independent nodes
        let alice = get_node_with_dependencies("alice", None);
        let bob = get_node_with_dependencies("bob", None);
        let nodes = [&alice, &bob];

        let levels = dependency_levels_among(&nodes).unwrap();
        let expected = vec![vec!["alice", "bob"]];

        verify_levels(levels, expected);

        // alice -> bob -> charlie
        let alice = get_node_with_dependencies("alice", None);
        let bob = get_node_with_dependencies("bob", Some(vec![&alice]));
        let charlie = get_node_with_dependencies("charlie", Some(vec![&bob]));
        let nodes = [&alice, &bob, &charlie];

        let levels = dependency_levels_among(&nodes).unwrap();
        let expected = vec![vec!["alice"], vec!["bob"], vec!["charlie"]];

        verify_levels(levels, expected);

        //         ┌─> bob
        // alice ──|
        //         └─> charlie
        let alice = get_node_with_dependencies("alice", None);
        let bob = get_node_with_dependencies("bob", Some(vec![&alice]));
        let charlie = get_node_with_dependencies("charlie", Some(vec![&alice]));
        let nodes = [&alice, &bob, &charlie];

        let levels = dependency_levels_among(&nodes).unwrap();
        let expected = vec![vec!["alice"], vec!["bob", "charlie"]];

        verify_levels(levels, expected);

        //         ┌─>   bob  ──┐
        // alice ──|            ├─> dave
        //         └─> charlie  ┘
        let alice = get_node_with_dependencies("alice", None);
        let bob = get_node_with_dependencies("bob", Some(vec![&alice]));
        let charlie = get_node_with_dependencies("charlie", Some(vec![&alice]));
        let dave = get_node_with_dependencies("dave", Some(vec![&charlie, &bob]));
        let nodes = [&alice, &bob, &charlie, &dave];

        let levels = dependency_levels_among(&nodes).unwrap();
        let expected = vec![vec!["alice"], vec!["bob", "charlie"], vec!["dave"]];

        verify_levels(levels, expected);
    }

    #[test]
    fn dependency_levels_among_should_detect_cycles() {
        let mut alice = get_node_with_dependencies("alice", None);
        let bob = get_node_with_dependencies("bob", Some(vec![&alice]));
        alice.args.push("{{ZOMBIE:bob:someField}}".into());

        assert!(dependency_levels_among(&[&alice, &bob]).is_err())
    }
}
