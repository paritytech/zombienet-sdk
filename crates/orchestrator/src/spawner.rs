use std::{collections::HashMap, net::IpAddr, path::PathBuf, sync::Arc};

use anyhow::Context;
use configuration::{
    types::{JamNodeMode, Port},
    CustomProcess, GlobalSettings,
};
use jam_std_common::hash_raw;
use provider::{
    constants::{
        LOCALHOST, NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, P2P_PORT, RPC_HTTP_PORT,
        RPC_WS_PORT,
    },
    shared::helpers::running_in_ci,
    types::{NodeRole, SpawnNodeOptions, TransferedFile},
    DynNamespace, DynNode, ProviderNamespace,
};
use support::{
    constants::THIS_IS_A_BUG,
    fs::FileSystem,
    replacer::{apply_running_network_replacements, has_tokens},
};
use tracing::{info, warn};

use crate::{
    generators::{self, ResolvedDbSnapshots},
    network::{
        node::{CustomProcessNode, JamNetworkNode, NetworkNode, ProcessPort},
        Network, NodeContext,
    },
    network_spec::{jamnode::JamNodeSpec, node::NodeSpec, parachain::ParachainSpec, NetworkSpec},
    shared::{
        constants::{FULL_NODE_PROMETHEUS_PORT, JAM_PORT, PROMETHEUS_PORT, RPC_PORT},
        types::ParkedPort,
    },
    ScopedFilesystem, ZombieRole,
};

/// The ports a node is reachable on inside the network (its k8s `Service`).
const NODE_SERVICE_PORTS: [(&str, Port); 4] = [
    ("p2p", P2P_PORT),
    ("rpc", RPC_WS_PORT),
    ("rpc-http", RPC_HTTP_PORT),
    ("prom", PROMETHEUS_PORT),
];

#[derive(Clone)]
pub struct SpawnNodeCtx<'a, T: FileSystem> {
    /// Relaychain id, from the chain-spec (e.g rococo_local_testnet)
    pub(crate) chain_id: &'a str,
    // Parachain id, from the chain-spec (e.g local_testnet)
    pub(crate) parachain_id: Option<&'a str>,
    /// Relaychain chain name (e.g rococo-local)
    pub(crate) chain: &'a str,
    /// Role of the node in the network
    pub(crate) role: ZombieRole,
    /// Ref to the namespace
    pub(crate) ns: &'a DynNamespace,
    /// Ref to an scoped filesystem (encapsulate fs actions inside the ns directory)
    pub(crate) scoped_fs: &'a ScopedFilesystem<'a, T>,
    /// Ref to a parachain (used to spawn collators)
    pub(crate) parachain: Option<&'a ParachainSpec>,
    /// The string representation of the bootnode address to pass to nodes
    pub(crate) bootnodes_addr: &'a Vec<String>,
    /// Flag to wait node is ready or not
    /// Ready state means we can query Prometheus internal server
    pub(crate) wait_ready: bool,
    /// A json representation of the running nodes with their names as 'key'
    pub(crate) nodes_by_name: serde_json::Value,
    /// A ref to the global settings
    pub(crate) global_settings: &'a GlobalSettings,
    /// `db_snapshot` `AssetLocation`s resolved to local cache paths,
    /// populated once before the parallel spawn fanout. The provider's
    /// `initialize_db_snapshot` reads from these paths only — no
    /// downloads happen inside the per-node spawn.
    pub(crate) resolved_db_snapshots: &'a ResolvedDbSnapshots,
}

pub async fn spawn_node<'a, T>(
    node: &NodeSpec,
    mut files_to_inject: Vec<TransferedFile>,
    ctx: &SpawnNodeCtx<'a, T>,
) -> Result<NetworkNode, anyhow::Error>
where
    T: FileSystem,
{
    let mut created_paths = vec![];
    // Create and inject the keystore IFF
    // - The node is validator in the relaychain
    // - The node is collator (encoded as validator) and the parachain is cumulus_based
    // (parachain_id) should be set then.
    if node.is_validator && (ctx.parachain.is_none() || ctx.parachain_id.is_some()) {
        // Generate keystore for node
        let node_files_path = if let Some(para) = ctx.parachain {
            para.id.to_string()
        } else {
            node.name.clone()
        };
        let asset_hub_polkadot = ctx
            .parachain_id
            .map(|id| id.starts_with("asset-hub-polkadot"))
            .unwrap_or_default();
        let keystore_key_types = node.keystore_key_types.iter().map(String::as_str).collect();
        let key_filenames = generators::generate_node_keystore(
            &node.accounts,
            &node_files_path,
            ctx.scoped_fs,
            asset_hub_polkadot,
            keystore_key_types,
        )
        .await
        .unwrap();

        // Paths returned are relative to the base dir, we need to convert into
        // fullpaths to inject them in the nodes.
        let remote_keystore_chain_id = if let Some(id) = ctx.parachain_id {
            id
        } else {
            ctx.chain_id
        };

        let keystore_path = node.keystore_path.clone().unwrap_or(PathBuf::from(format!(
            "/data/chains/{remote_keystore_chain_id}/keystore",
        )));

        for key_filename in key_filenames {
            let f = TransferedFile::new(
                PathBuf::from(format!(
                    "{}/{}/{}",
                    ctx.ns.base_dir().to_string_lossy(),
                    node_files_path,
                    key_filename.to_string_lossy()
                )),
                keystore_path.join(key_filename),
            );
            files_to_inject.push(f);
        }
        created_paths.push(keystore_path);
    }

    let base_dir = format!("{}/{}", ctx.ns.base_dir().to_string_lossy(), node.name);

    let (cfg_path, data_path, relay_data_path) = if !ctx.ns.capabilities().prefix_with_full_path {
        (
            NODE_CONFIG_DIR.into(),
            NODE_DATA_DIR.into(),
            NODE_RELAY_DATA_DIR.into(),
        )
    } else {
        let cfg_path = format!("{}{NODE_CONFIG_DIR}", base_dir);
        let data_path = format!("{}{NODE_DATA_DIR}", base_dir);
        let relay_data_path = format!("{}{NODE_RELAY_DATA_DIR}", base_dir);
        (cfg_path, data_path, relay_data_path)
    };

    let gen_opts = generators::GenCmdOptions {
        relay_chain_name: ctx.chain.to_string(),
        cfg_path,           // TODO: get from provider/ns
        data_path,          // TODO: get from provider
        relay_data_path,    // TODO: get from provider
        use_wrapper: false, // TODO: get from provider
        bootnode_addr: ctx.bootnodes_addr.clone(),
        use_default_ports_in_cmd: ctx.ns.capabilities().use_default_ports_in_cmd,
        // IFF the provider require an image (e.g k8s) we know this is not native
        is_native: !ctx.ns.capabilities().requires_image,
    };

    let mut collator_full_node_prom_port: Option<u16> = None;
    let mut collator_full_node_prom_port_external: Option<u16> = None;

    let (program, args) = match ctx.role {
        // Collator should be `non-cumulus` one (e.g adder/undying)
        ZombieRole::Node | ZombieRole::Collator => {
            let maybe_para_id = ctx.parachain.map(|para| para.id);

            generators::generate_node_command(node, gen_opts.clone(), maybe_para_id)
        },
        ZombieRole::CumulusCollator => {
            let para = ctx.parachain.expect(&format!(
                "parachain must be part of the context {THIS_IS_A_BUG}"
            ));
            collator_full_node_prom_port = node.full_node_prometheus_port.as_ref().map(|p| p.0);

            generators::generate_node_command_cumulus(node, gen_opts.clone(), para.id)
        },
        _ => unreachable!(), /* TODO: do we need those?
                              * ZombieRole::Bootnode => todo!(), */
    };

    // apply running networ replacements
    let args: Vec<String> = args
        .iter()
        .map(|arg| apply_running_network_replacements(arg, &ctx.nodes_by_name))
        .collect();

    info!(
        "🚀 {}, spawning.... with command: {} {}",
        node.name,
        program,
        args.join(" ")
    );

    let ports = if ctx.ns.capabilities().use_default_ports_in_cmd {
        // should use default ports to as internal
        [
            (P2P_PORT, node.p2p_port.0),
            (RPC_PORT, node.rpc_port.0),
            (PROMETHEUS_PORT, node.prometheus_port.0),
        ]
    } else {
        [
            (P2P_PORT, P2P_PORT),
            (RPC_PORT, RPC_PORT),
            (PROMETHEUS_PORT, PROMETHEUS_PORT),
        ]
    };

    let resolved_db_snapshot = node
        .db_snapshot
        .as_ref()
        .and_then(|loc| ctx.resolved_db_snapshots.get(loc).cloned());
    if node.db_snapshot.is_some() && resolved_db_snapshot.is_none() {
        // Invariant: every NodeSpec.db_snapshot must have been resolved
        // by orchestrator::generators::resolve_db_snapshots before we
        // get here. Hitting this path means the resolution step was
        // skipped for this node.
        return Err(anyhow::anyhow!(
            "{THIS_IS_A_BUG}: node {} has db_snapshot {:?} but no resolved cache entry",
            node.name,
            node.db_snapshot,
        ));
    }

    let spawn_ops = SpawnNodeOptions::new(node.name.clone(), program)
        .args(args)
        .env(
            node.env
                .iter()
                .map(|var| (var.name.clone(), var.value.clone())),
        )
        .injected_files(files_to_inject)
        .created_paths(created_paths)
        .db_snapshot(resolved_db_snapshot)
        .port_mapping(HashMap::from(ports))
        .ports(NODE_SERVICE_PORTS)
        .node_log_path(node.node_log_path.clone())
        .role(ctx.role.node_role());

    let spawn_ops = if let Some(image) = node.image.as_ref() {
        spawn_ops.image(image.as_str())
    } else {
        spawn_ops
    };

    let spawn_ops = if let Some(resources) = node.resources.as_ref() {
        spawn_ops.resources(resources.clone())
    } else {
        spawn_ops
    };

    // Drops the port parking listeners before spawn
    node.ws_port.drop_listener();
    node.p2p_port.drop_listener();
    node.rpc_port.drop_listener();
    node.prometheus_port.drop_listener();
    if let Some(port) = &node.full_node_p2p_port {
        port.drop_listener();
    }
    if let Some(port) = &node.full_node_prometheus_port {
        port.drop_listener();
    }

    let running_node = ctx.ns.spawn_node(&spawn_ops).await.with_context(|| {
        format!(
            "Failed to spawn node: {} with opts: {:#?}",
            node.name, spawn_ops
        )
    })?;

    let mut ip_to_use = if let Some(local_ip) = ctx.global_settings.local_ip() {
        *local_ip
    } else {
        LOCALHOST
    };

    let (rpc_port_external, prometheus_port_external, p2p_external);

    if running_in_ci() && ctx.ns.provider_name() == "k8s" {
        // running kubernets in ci require to use ip and default port
        (rpc_port_external, prometheus_port_external, p2p_external) =
            (RPC_PORT, PROMETHEUS_PORT, P2P_PORT);
        collator_full_node_prom_port_external = Some(FULL_NODE_PROMETHEUS_PORT);
        ip_to_use = running_node.ip().await?;
    } else {
        // Create port-forward iff we are not in CI or provider doesn't use the default ports (native)
        let ports = futures::future::try_join_all(vec![
            running_node.create_port_forward(node.rpc_port.0, RPC_PORT),
            running_node.create_port_forward(node.prometheus_port.0, PROMETHEUS_PORT),
        ])
        .await?;

        (rpc_port_external, prometheus_port_external, p2p_external) = (
            ports[0].unwrap_or(node.rpc_port.0),
            ports[1].unwrap_or(node.prometheus_port.0),
            // p2p don't need port-fwd
            node.p2p_port.0,
        );

        if let Some(full_node_prom_port) = collator_full_node_prom_port {
            let port_fwd = running_node
                .create_port_forward(full_node_prom_port, FULL_NODE_PROMETHEUS_PORT)
                .await?;
            collator_full_node_prom_port_external = Some(port_fwd.unwrap_or(full_node_prom_port));
        }
    }

    let running_ip = running_node.ip().await?;

    let multiaddr = generators::generate_node_bootnode_addr(
        &node.peer_id,
        &running_ip,
        if ctx.ns.provider_name() == "k8s" {
            P2P_PORT
        } else {
            p2p_external
        }, // for k8s use always the internal port
        running_node.args().as_ref(),
        &node.p2p_cert_hash,
    )?;

    let ws_uri = format!("ws://{ip_to_use}:{rpc_port_external}");
    let internal_ws_uri = if ctx.ns.provider_name() == "k8s" {
        format!("ws://{running_ip}:{RPC_PORT}")
    } else {
        // docker/native can use the same
        ws_uri.clone()
    };
    let prometheus_uri = format!("http://{ip_to_use}:{prometheus_port_external}/metrics");
    info!("🚀 {}, should be running now", node.name);
    info!(
        "💻 {}: direct link (pjs) https://polkadot.js.org/apps/?rpc={ws_uri}#/explorer",
        node.name
    );
    info!(
        "💻 {}: direct link (papi) https://dev.papi.how/explorer#networkId=custom&endpoint={ws_uri}",
        node.name
    );

    info!("📊 {}: metrics link {prometheus_uri}", node.name);

    if let Some(full_node_prom_port) = collator_full_node_prom_port_external {
        info!(
            "📊 {}: collator full-node metrics link http://{}:{}/metrics",
            node.name, ip_to_use, full_node_prom_port
        );
    }

    info!("📓 logs cmd: {}", running_node.log_cmd());

    let node_ctx = if let Some(parachain) = ctx.parachain {
        NodeContext::Para {
            para_id: parachain.id,
            is_cumulus_based: parachain.is_cumulus_based,
        }
    } else {
        NodeContext::Rc
    };

    Ok(NetworkNode::new(
        node.name.clone(),
        ws_uri,
        internal_ws_uri,
        prometheus_uri,
        multiaddr,
        node.clone(),
        running_node,
        gen_opts,
        node_ctx,
    ))
}

/// The `args` and `env` of a custom process, placeholders resolved.
type ResolvedArgsAndEnv = (Vec<String>, Vec<(String, String)>);

/// The args and env of `custom_process` with every `{{ZOMBIE:<name>:<field>}}`
/// resolved against the nodes already running (`nodes_by_name`, as the
/// orchestrator keeps them) and the process's own ports, as `port_<name>`.
///
/// A placeholder that stays unresolved is an error: the process would start
/// with the literal text.
fn resolve_placeholders(
    custom_process: &CustomProcess,
    resolved_ports: &[(String, Port)],
    nodes_by_name: &serde_json::Value,
) -> Result<ResolvedArgsAndEnv, anyhow::Error> {
    let name = custom_process.name();
    let mut context = nodes_by_name.clone();
    context[name] = serde_json::json!(resolved_ports
        .iter()
        .map(|(port_name, port)| (format!("port_{port_name}"), port.to_string()))
        .collect::<HashMap<_, _>>());

    let resolve = |text: &str| -> Result<String, anyhow::Error> {
        let resolved = apply_running_network_replacements(text, &context);
        if has_tokens(&resolved) {
            Err(anyhow::anyhow!(
                "{name}: unresolved placeholder in {resolved:?} (nodes in context: {})",
                context
                    .as_object()
                    .map(|nodes| nodes.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            ))
        } else {
            Ok(resolved)
        }
    };

    let args = custom_process
        .args()
        .iter()
        .flat_map(|arg| arg.to_vec())
        .map(|arg| resolve(&arg))
        .collect::<Result<Vec<_>, _>>()?;
    let env = custom_process
        .env()
        .iter()
        .map(|var| Ok((var.name.clone(), resolve(&var.value)?)))
        .collect::<Result<Vec<_>, anyhow::Error>>()?;

    Ok((args, env))
}

/// A custom process with its ports parked, ready to spawn.
///
/// Parking happens for every process before any of them spawns, so a port one
/// process picks (or declares, on native) cannot be handed to another; each
/// process releases its own right before its spawn.
pub(crate) struct PreparedProcess<'a> {
    spec: &'a CustomProcess,
    /// Every declared port by name, `0`s resolved to the picked number.
    ports: Vec<(String, Port)>,
    /// On docker, the host port each port is published on.
    host_ports: HashMap<Port, Port>,
    parked: Vec<ParkedPort>,
}

/// Resolve and park the ports of `custom_process`: a `0` is picked free, a
/// fixed one on native has to be free too (or the process loses the bind and
/// whoever holds the port answers for it), and on docker each port gets a
/// parked host port to be published on, so two networks can share a host.
pub(crate) fn prepare_process<'a>(
    custom_process: &'a CustomProcess,
    ns: &dyn ProviderNamespace,
) -> Result<PreparedProcess<'a>, anyhow::Error> {
    let name = custom_process.name();
    let mut ports: Vec<(String, Port)> = Vec::with_capacity(custom_process.ports().len());
    let mut host_ports: HashMap<Port, Port> = HashMap::new();
    let mut parked = vec![];
    // A picked port may not land on a fixed one of the same process.
    let fixed: Vec<Port> = custom_process
        .ports()
        .iter()
        .map(|p| p.port)
        .filter(|p| *p != 0)
        .collect();
    for declared in custom_process.ports() {
        let port = if declared.port == 0 {
            let picked = loop {
                let picked = generators::generate_node_port(None)?;
                if !fixed.contains(&picked.0) {
                    break picked;
                }
            };
            let port = picked.0;
            parked.push(picked);
            port
        } else if !ns.capabilities().requires_image {
            let picked =
                generators::generate_node_port(Some(declared.port)).with_context(|| {
                    format!(
                        "{name}: port {} ('{}') is already in use",
                        declared.port, declared.name
                    )
                })?;
            parked.push(picked);
            declared.port
        } else {
            declared.port
        };
        if ns.provider_name() == "docker" {
            let host = generators::generate_node_port(None)?;
            host_ports.insert(port, host.0);
            parked.push(host);
        }
        ports.push((declared.name.clone(), port));
    }

    Ok(PreparedProcess {
        spec: custom_process,
        ports,
        host_ports,
        parked,
    })
}

/// Spawn a custom process and make it a member of the network: the running
/// handle is returned with every declared port resolved to where it is
/// reachable, so it can be tracked, written to `zombie.json` and reattached.
///
/// The process runs without zombienet's wrapper script: it is an
/// off-the-shelf image most of the time, which may lack `bash`, and the
/// pause/resume controls the wrapper provides are for nodes.
///
/// Every port is offered to the process as `{{ZOMBIE:<process>:port_<name>}}`
/// in its args and env.
pub(crate) async fn spawn_process(
    prepared: PreparedProcess<'_>,
    ns: Arc<dyn ProviderNamespace + Send + Sync>,
    host_ip: IpAddr,
    nodes_by_name: &serde_json::Value,
) -> Result<CustomProcessNode, anyhow::Error> {
    let PreparedProcess {
        spec: custom_process,
        ports: resolved_ports,
        host_ports,
        parked,
    } = prepared;
    let provider = ns.provider_name();
    let capabilities = ns.capabilities();
    let name = custom_process.name();

    let (args, env) = resolve_placeholders(custom_process, &resolved_ports, nodes_by_name)?;

    let spawn_ops = SpawnNodeOptions::new(name, custom_process.command().as_str())
        .args(&args)
        .env(env)
        .ports(resolved_ports.iter().map(|(n, p)| (n.as_str(), *p)))
        // only docker publishes ports; k8s and native ignore the mapping
        .port_mapping(host_ports.clone())
        .role(NodeRole::CustomProcess)
        .without_wrapper();

    let spawn_ops = if let Some(image) = custom_process.image() {
        spawn_ops.image(image.as_str())
    } else {
        spawn_ops
    };

    let spawn_ops = match custom_process.resources() {
        Some(resources) if capabilities.has_resources => spawn_ops.resources(resources.clone()),
        Some(_) => {
            warn!(
                "⚠️  {name}: resources are not supported by the {provider} provider, ignoring them"
            );
            spawn_ops
        },
        None => spawn_ops,
    };

    info!(
        "🚀 {name}, spawning custom process.... with command: {} {}",
        custom_process.command().as_str(),
        args.join(" ")
    );

    for port in &parked {
        port.drop_listener();
    }

    let running_node = ns
        .spawn_node(&spawn_ops)
        .await
        .with_context(|| format!("Failed to spawn node: {name} with opts: {:#?}", spawn_ops))?;

    // From here on the process runs. Nothing below may lose it: a port we
    // cannot reach from here is reported by its in-network address instead.
    let running_ip = match running_node.ip().await {
        Ok(ip) => ip,
        Err(err) => {
            warn!("⚠️  {name}: could not get the process ip ({err}); using {host_ip}");
            host_ip
        },
    };

    let in_ci = running_in_ci();
    let mut ports = std::collections::BTreeMap::new();
    for (port_name, port) in &resolved_ports {
        // `internal` is the address inside the network, `external` the one
        // from where zombienet runs.
        let (internal, external) = match provider {
            // The Service named after the process; the pod itself from inside
            // the cluster (CI), a port-forward on this host otherwise.
            "k8s" => {
                let internal = format!("{name}:{port}");
                let external = if in_ci {
                    format!("{running_ip}:{port}")
                } else {
                    forward_or_keep(&running_node, name, port_name, *port, internal.clone()).await
                };
                (internal, external)
            },
            // The container; the port it is published on.
            "docker" => (
                format!("{running_ip}:{port}"),
                format!("{host_ip}:{}", host_ports[port]),
            ),
            // The host, both ways.
            _ => (format!("{host_ip}:{port}"), format!("{host_ip}:{port}")),
        };

        ports.insert(
            port_name.clone(),
            ProcessPort {
                port: *port,
                external,
                internal,
            },
        );
    }

    info!("🚀 {name}, should be running now");
    for (port_name, port) in &ports {
        info!(
            "🔌 {name}: port {port_name} ({}) reachable at {}",
            port.port, port.external
        );
    }
    info!("📓 logs cmd: {}", running_node.log_cmd());

    Ok(CustomProcessNode::new(
        name,
        running_node,
        custom_process.clone(),
        running_ip,
        ports,
    ))
}

/// A port-forward to `port` of `node`, as `host:port` on this host, or
/// `fallback` where the provider has none to offer or it could not be opened.
pub(crate) async fn forward_or_keep(
    node: &DynNode,
    process: &str,
    port_name: &str,
    port: Port,
    fallback: String,
) -> String {
    match node.create_port_forward(0, port).await {
        Ok(Some(local_port)) => format!("{LOCALHOST}:{local_port}"),
        Ok(None) => fallback,
        Err(err) => {
            warn!("⚠️  {process}: could not forward port {port_name} ({port}): {err}");
            fallback
        },
    }
}

/// Spawn the custom processes of `spec`, concurrently, and register the ones
/// that came up. A process that does not is logged and left out: the nodes
/// are up by now, and a missing side service must not take the network down.
pub(crate) async fn spawn_custom_processes<T: FileSystem>(
    network: &mut Network<T>,
    spec: &NetworkSpec,
    ns: Arc<dyn ProviderNamespace + Send + Sync>,
) -> Result<(), anyhow::Error> {
    if spec.custom_processes.is_empty() {
        return Ok(());
    }
    let host_ip = spec
        .global_settings
        .local_ip()
        .copied()
        .unwrap_or(LOCALHOST);
    let nodes_by_name = network.nodes_json()?;

    // Checked before spawning: a process that ran and was then refused would
    // be untracked, having taken the node's place in the provider's registry.
    let (taken, free): (Vec<_>, Vec<_>) = spec
        .custom_processes
        .iter()
        .partition(|cp| network.has_member(cp.name()));
    for cp in taken {
        warn!(
            "⚠️  Custom process {} not spawned: the name is already taken by a node or custom process",
            cp.name()
        );
    }

    // Every process parks its ports before any spawns, see `PreparedProcess`.
    let prepared: Vec<PreparedProcess> = free
        .into_iter()
        .filter_map(|cp| match prepare_process(cp, ns.as_ref()) {
            Ok(prepared) => Some(prepared),
            Err(e) => {
                warn!(
                    "⚠️  Failed to reserve the ports of custom process {}, not spawned, err: {e}",
                    cp.name()
                );
                None
            },
        })
        .collect();
    let names: Vec<&str> = prepared.iter().map(|p| p.spec.name()).collect();
    let spawning = prepared
        .into_iter()
        .map(|p| spawn_process(p, ns.clone(), host_ip, &nodes_by_name));
    for (name, spawned) in names
        .into_iter()
        .zip(futures::future::join_all(spawning).await)
    {
        match spawned {
            Ok(process) => network.add_running_custom_process(process),
            Err(e) => warn!("⚠️  Failed to spawn custom process {name}, err: {e}"),
        }
    }

    Ok(())
}

pub async fn spawn_jam_node<'a, T>(
    node: &JamNodeSpec,
    mut files_to_inject: Vec<TransferedFile>,
    ctx: &SpawnNodeCtx<'a, T>,
) -> Result<JamNetworkNode, anyhow::Error>
where
    T: FileSystem,
{
    let base_dir = format!("{}/{}", ctx.ns.base_dir().to_string_lossy(), node.name);

    let (cfg_path, data_path) = if !ctx.ns.capabilities().prefix_with_full_path {
        (NODE_CONFIG_DIR.into(), NODE_DATA_DIR.into())
    } else {
        let cfg_path = format!("{}{NODE_CONFIG_DIR}", base_dir);
        let data_path = format!("{}{NODE_DATA_DIR}", base_dir);
        (cfg_path, data_path)
    };

    // create local seed file and set to transfer process:

    // 1. Create local dir to store all the needed files
    ctx.scoped_fs
        .create_dir_all(PathBuf::from(&node.name))
        .await?;

    // 2. set paths to use (local and remote)
    let keys_remote = PathBuf::from(format!("{}/{}/keys", NODE_CONFIG_DIR, ctx.chain));
    let seed_local_path = PathBuf::from(format!("{}/{}.seed", base_dir, node.name));
    let seed_remote_path = PathBuf::from(format!(
        "{}/{}.seed",
        keys_remote.to_string_lossy(),
        node.name
    ));

    // 3. write local seed
    ctx.scoped_fs
        .write(&seed_local_path, hash_raw(node.accounts.seed.as_bytes()))
        .await?;

    // 4. set paths/files to use in remote.
    let created_paths = vec![keys_remote];
    files_to_inject.push(TransferedFile::new(seed_local_path, seed_remote_path));

    let gen_opts = generators::GenCmdOptions {
        relay_chain_name: ctx.chain.to_string(),
        cfg_path,  // TODO: get from provider/ns
        data_path, // TODO: get from provider
        use_default_ports_in_cmd: ctx.ns.capabilities().use_default_ports_in_cmd,
        // IFF the provider require an image (e.g k8s) we know this is not native
        is_native: !ctx.ns.capabilities().requires_image,
        bootnode_addr: ctx.bootnodes_addr.clone(),
        ..Default::default()
    };

    let (program, args) = generators::generate_jam_node_command(node, gen_opts.clone());
    // apply running networ replacements
    let args: Vec<String> = args
        .iter()
        .map(|arg| apply_running_network_replacements(arg, &ctx.nodes_by_name))
        .collect();

    info!(
        "🚀 {}, spawning.... with command: {} {}",
        node.name,
        program,
        args.join(" ")
    );

    let ports = if ctx.ns.capabilities().use_default_ports_in_cmd {
        // should use default ports to as internal
        [(JAM_PORT, node.port.0), (RPC_PORT, node.rpc_port.0)]
    } else {
        [(JAM_PORT, JAM_PORT), (RPC_PORT, RPC_PORT)]
    };

    let spawn_ops = SpawnNodeOptions::new(node.name.clone(), program)
        .args(args)
        .env(
            node.env
                .iter()
                .map(|var| (var.name.clone(), var.value.clone())),
        )
        .injected_files(files_to_inject)
        .created_paths(created_paths)
        .port_mapping(HashMap::from(ports))
        .ports(NODE_SERVICE_PORTS);

    let spawn_ops = if let Some(image) = node.image.as_ref() {
        spawn_ops.image(image.as_str())
    } else {
        spawn_ops
    };

    let spawn_ops = if let Some(resources) = node.resources.as_ref() {
        spawn_ops.resources(resources.clone())
    } else {
        spawn_ops
    };

    // Drops the port parking listeners before spawn
    node.port.drop_listener();
    node.rpc_port.drop_listener();

    let running_node = ctx.ns.spawn_node(&spawn_ops).await.with_context(|| {
        format!(
            "Failed to spawn node: {} with opts: {:#?}",
            node.name, spawn_ops
        )
    })?;

    let ip_to_use = if let Some(local_ip) = ctx.global_settings.local_ip() {
        *local_ip
    } else {
        LOCALHOST
    };

    // NOTE: running in ci (k8s) is not supported yet for JAM, so we don't
    // create port-forwards nor use the internal ip/default ports here.

    info!("🚀 {}, should be running now", node.name);
    match node.mode {
        JamNodeMode::Ordinary => {
            info!("💻 {}, rpc  {ip_to_use}:{}", node.name, node.rpc_port.0);
        },
        JamNodeMode::Validator | JamNodeMode::Proxy => {
            info!(
                "💻 {}, peer details {}@{ip_to_use}:{}",
                node.name, node.peer_id, node.port.0
            );
        },
    }

    info!("📓 logs cmd: {}", running_node.log_cmd());

    Ok(JamNetworkNode::new(
        node.name.clone(),
        running_node,
        node.clone(),
        ip_to_use,
        gen_opts,
    ))
}

#[cfg(test)]
mod tests {
    use configuration::CustomProcessBuilder;

    use super::*;

    fn eth_rpc() -> CustomProcess {
        CustomProcessBuilder::new()
            .with_name("eth-rpc")
            .with_command("eth-rpc")
            .with_args(vec![
                ("--node-rpc-url", "{{ZOMBIE:asset-hub-1:internal_ws_uri}}").into(),
                ("--rpc-port", "{{ZOMBIE:eth-rpc:port_http}}").into(),
                "--rpc-external".into(),
            ])
            .with_env(vec![
                ("RUST_LOG", "info"),
                ("UPSTREAM", "{{ZOMBIE:asset-hub-1:ws_uri}}"),
            ])
            .with_port("http", 0)
            .build()
            .unwrap()
    }

    fn nodes() -> serde_json::Value {
        serde_json::json!({
            "asset-hub-1": {
                "name": "asset-hub-1",
                "ws_uri": "ws://127.0.0.1:51234",
                "internal_ws_uri": "ws://asset-hub-1:9944",
            }
        })
    }

    #[test]
    fn placeholders_resolve_nodes_and_own_ports() {
        let (args, env) =
            resolve_placeholders(&eth_rpc(), &[("http".into(), 8545)], &nodes()).unwrap();

        assert_eq!(
            args,
            vec![
                "--node-rpc-url",
                "ws://asset-hub-1:9944",
                "--rpc-port",
                "8545",
                "--rpc-external"
            ]
        );
        assert_eq!(
            env,
            vec![
                ("RUST_LOG".to_string(), "info".to_string()),
                ("UPSTREAM".to_string(), "ws://127.0.0.1:51234".to_string()),
            ]
        );
    }

    #[test]
    fn an_unresolved_placeholder_is_an_error() {
        // no such port
        let err = resolve_placeholders(&eth_rpc(), &[("rpc".into(), 8545)], &nodes()).unwrap_err();
        assert!(err.to_string().contains("port_http"), "{err}");

        // no such node
        let err =
            resolve_placeholders(&eth_rpc(), &[("http".into(), 8545)], &serde_json::json!({}))
                .unwrap_err();
        assert!(err.to_string().contains("asset-hub-1"), "{err}");
    }
}
