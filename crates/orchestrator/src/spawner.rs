use std::{collections::HashMap, path::PathBuf};

use anyhow::Context;
use configuration::GlobalSettings;
use provider::{
    constants::{LOCALHOST, NODE_CONFIG_DIR, NODE_DATA_DIR, NODE_RELAY_DATA_DIR, P2P_PORT},
    shared::helpers::running_in_ci,
    types::{SpawnNodeOptions, TransferedFile},
    DynNamespace,
};
use support::{
    constants::THIS_IS_A_BUG, fs::FileSystem, replacer::apply_running_network_replacements,
};
use tracing::info;

use crate::{
    generators,
    network::node::NetworkNode,
    network_spec::{node::NodeSpec, parachain::ParachainSpec},
    shared::constants::{FULL_NODE_PROMETHEUS_PORT, PROMETHEUS_PORT, RPC_PORT},
    ScopedFilesystem, ZombieRole,
};

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
        let key_filenames = generators::generate_node_keystore(
            &node.accounts,
            &node_files_path,
            ctx.scoped_fs,
            asset_hub_polkadot,
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

    let base_dir = format!("{}/{}", ctx.ns.base_dir().to_string_lossy(), &node.name);

    let (cfg_path, data_path, relay_data_path) = if !ctx.ns.capabilities().prefix_with_full_path {
        (
            NODE_CONFIG_DIR.into(),
            NODE_DATA_DIR.into(),
            NODE_RELAY_DATA_DIR.into(),
        )
    } else {
        let cfg_path = format!("{}{NODE_CONFIG_DIR}", &base_dir);
        let data_path = format!("{}{NODE_DATA_DIR}", &base_dir);
        let relay_data_path = format!("{}{NODE_RELAY_DATA_DIR}", &base_dir);
        (cfg_path, data_path, relay_data_path)
    };

    let gen_opts = generators::GenCmdOptions {
        relay_chain_name: ctx.chain,
        cfg_path: &cfg_path,               // TODO: get from provider/ns
        data_path: &data_path,             // TODO: get from provider
        relay_data_path: &relay_data_path, // TODO: get from provider
        use_wrapper: false,                // TODO: get from provider
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

            generators::generate_node_command(node, gen_opts, maybe_para_id)
        },
        ZombieRole::CumulusCollator => {
            let para = ctx.parachain.expect(&format!(
                "parachain must be part of the context {THIS_IS_A_BUG}"
            ));
            collator_full_node_prom_port = node.full_node_prometheus_port.as_ref().map(|p| p.0);

            generators::generate_node_command_cumulus(node, gen_opts, para.id)
        },
        _ => unreachable!(), /* TODO: do we need those?
                              * ZombieRole::Bootnode => todo!(),
                              * ZombieRole::Companion => todo!(), */
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

    let spawn_ops = SpawnNodeOptions::new(node.name.clone(), program)
        .args(args)
        .env(
            node.env
                .iter()
                .map(|var| (var.name.clone(), var.value.clone())),
        )
        .injected_files(files_to_inject)
        .created_paths(created_paths)
        .db_snapshot(node.db_snapshot.clone())
        .port_mapping(HashMap::from(ports))
        .node_log_path(node.node_log_path.clone());

    let spawn_ops = if let Some(image) = node.image.as_ref() {
        spawn_ops.image(image.as_str())
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

    let multiaddr = generators::generate_node_bootnode_addr(
        &node.peer_id,
        &running_node.ip().await?,
        p2p_external,
        running_node.args().as_ref(),
        &node.p2p_cert_hash,
    )?;

    let ws_uri = format!("ws://{ip_to_use}:{rpc_port_external}");
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

    Ok(NetworkNode::new(
        node.name.clone(),
        ws_uri,
        prometheus_uri,
        multiaddr,
        node.clone(),
        running_node,
    ))
}
