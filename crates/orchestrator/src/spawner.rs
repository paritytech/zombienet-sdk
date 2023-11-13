use std::path::PathBuf;

use provider::{
    constants::LOCALHOST,
    types::{SpawnNodeOptions, TransferedFile},
    DynNamespace,
};
use support::fs::FileSystem;

use crate::{
    generators,
    network::node::NetworkNode,
    network_spec::{node::NodeSpec, parachain::ParachainSpec},
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
    /// The string represenation of the bootnode addres to pass to nodes
    pub(crate) bootnodes_addr: &'a Vec<String>,
    /// Flag to wait node is ready or not
    /// Ready state means we can query prometheus internal server
    pub(crate) wait_ready: bool,
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
        let key_filenames =
            generators::generate_node_keystore(&node.accounts, &node_files_path, ctx.scoped_fs)
                .await
                .unwrap();

        // Paths returned are relative to the base dir, we need to convert into
        // fullpaths to inject them in the nodes.
        let remote_keystore_chain_id = if let Some(id) = ctx.parachain_id {
            id
        } else {
            ctx.chain_id
        };

        for key_filename in key_filenames {
            let f = TransferedFile::new(
                PathBuf::from(format!(
                    "{}/{}/{}",
                    ctx.ns.base_dir().to_string_lossy(),
                    node_files_path,
                    key_filename.to_string_lossy()
                )),
                PathBuf::from(format!(
                    "/data/chains/{}/keystore/{}",
                    remote_keystore_chain_id,
                    key_filename.to_string_lossy()
                )),
            );
            files_to_inject.push(f);
        }
        created_paths.push(PathBuf::from(format!(
            "/data/chains/{}/keystore",
            remote_keystore_chain_id
        )));
    }

    let base_dir = format!("{}/{}", ctx.ns.base_dir().to_string_lossy(), &node.name);
    let cfg_path = format!("{}/cfg", &base_dir);
    let data_path = format!("{}/data", &base_dir);
    let relay_data_path = format!("{}/relay-data", &base_dir);
    let gen_opts = generators::GenCmdOptions {
        relay_chain_name: ctx.chain,
        cfg_path: &cfg_path,               // TODO: get from provider/ns
        data_path: &data_path,             // TODO: get from provider
        relay_data_path: &relay_data_path, // TODO: get from provider
        use_wrapper: false,                // TODO: get from provider
        bootnode_addr: ctx.bootnodes_addr.clone(),
    };

    let (program, args) = match ctx.role {
        // Collator should be `non-cumulus` one (e.g adder/undying)
        ZombieRole::Node | ZombieRole::Collator => {
            let maybe_para_id = ctx.parachain.map(|para| para.id);

            generators::generate_node_command(node, gen_opts, maybe_para_id)
        },
        ZombieRole::CumulusCollator => {
            let para = ctx
                .parachain
                .expect("parachain must be part of the context, this is a bug");
            let full_p2p = generators::generate_node_port(None)?;
            generators::generate_node_command_cumulus(node, gen_opts, para.id, full_p2p.0)
        },
        _ => unreachable!(), /* TODO: do we need those?
                              * ZombieRole::Bootnode => todo!(),
                              * ZombieRole::Companion => todo!(), */
    };

    println!("\n");
    println!("🚀 {}, spawning.... with command:", node.name);
    println!("{program} {}", args.join(" "));

    let spawn_ops = SpawnNodeOptions::new(node.name.clone(), program)
        .args(args)
        .env(
            node.env
                .iter()
                .map(|var| (var.name.clone(), var.value.clone())),
        )
        .injected_files(files_to_inject)
        .created_paths(created_paths);

    // Drops the port parking listeners before spawn
    node.p2p_port.drop_listener();
    node.rpc_port.drop_listener();
    node.prometheus_port.drop_listener();

    let running_node = ctx.ns.spawn_node(spawn_ops).await?;

    let ws_uri = format!("ws://{}:{}", LOCALHOST, node.rpc_port.0);
    let prometheus_uri = format!("http://{}:{}/metrics", LOCALHOST, node.prometheus_port.0);
    println!("🚀 {}, should be running now", node.name);
    println!(
        "🚀 {} : direct link https://polkadot.js.org/apps/?rpc={ws_uri}#/explorer",
        node.name
    );
    println!("🚀 {} : metrics link {prometheus_uri}", node.name);
    println!("📓 logs cmd: tail -f {}/{}.log", base_dir, node.name);
    println!("\n");
    Ok(NetworkNode::new(
        node.name.clone(),
        ws_uri,
        prometheus_uri,
        node.clone(),
        running_node,
    ))
}
