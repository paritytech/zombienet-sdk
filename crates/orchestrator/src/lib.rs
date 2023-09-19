// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code)]

mod errors;
mod generators;
mod network_spec;
mod shared;

use std::{time::Duration, path::{PathBuf, Path}};

use configuration::NetworkConfig;
use errors::OrchestratorError;
use network_spec::{NetworkSpec, node::NodeSpec, parachain::ParachainSpec};
use provider::{Provider, types::{TransferedFile, SpawnNodeOptions}, DynNamespace, DynNode, ProviderError, constants::LOCALHOST};
use support::fs::{FileSystem, FileSystemError};
use tokio::time::timeout;


pub struct Orchestrator<T, P>
where
    T: FileSystem + Sync + Send,
    P: Provider,
{
    filesystem: T,
    provider: P
}

impl<T, P> Orchestrator<T, P>
where
    T: FileSystem + Sync + Send + Clone,
    P: Provider,
{
    pub fn new(filesystem: T, provider:  P) -> Self {
        Self {
            filesystem,
            provider,
        }
    }

    pub async fn spawn(&self, network_config: NetworkConfig) -> Result<(), OrchestratorError> {
        let global_timeout = network_config.global_settings().network_spawn_timeout();
        let network_spec = NetworkSpec::from_config(&network_config).await?;

        timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_spec),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout))?
    }

    async fn spawn_inner(&self, mut network_spec: NetworkSpec) -> Result<(), OrchestratorError> {
        // main driver for spawn the network
        println!("{:#?}", network_spec);
        // create namespace
        let ns = self.provider.create_namespace().await?;


        println!("{:#?}", ns.id());
        println!("{:#?}", ns.base_dir());

        // Static setup
        // ns.static_setup().await?;

        let base_dir = ns.base_dir();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);
        // Creta chain-spec for relaychain
        network_spec.relaychain.chain_spec.build(&ns, &scoped_fs).await?;

        println!("{:#?}", network_spec.relaychain.chain_spec);

        // Create parachain artifacts (chain-spec, wasm, state)

        // Customize relaychain
        network_spec.relaychain.chain_spec.customize_relay(&network_spec.relaychain, &network_spec.hrmp_channels, vec![], &scoped_fs).await?;

        // Build raw version
        network_spec.relaychain.chain_spec.build_raw(&ns).await?;
        println!("{:#?}", network_spec.relaychain.chain_spec);

        // get the bootnodes to spawn first and calculate the bootnode string for use later
        let mut bootnodes = vec![];
        let mut relaynodes = vec![];
        network_spec.relaychain.nodes.iter().for_each(|node|{
            if node.is_bootnode { bootnodes.push(node) } else { relaynodes.push(node) }
        });

        if bootnodes.is_empty() {
            bootnodes.push(relaynodes.remove(0))
        }



        // Spawn first node of relay-chain
        // TODO: we want to still supporting spawn a dedicated bootnode??
        //let first_node = network_spec.relaychain.nodes.get(0).ok_or(OrchestratorError::InvalidConfig("At least one relaychain node is required".into()))?;
        let mut ctx = SpawnNodeCtx {
            chain_id: "rococo_local_testnet",
            chain: "rococo-local",
            role: ZombieRole::Node,
            ns: &ns,
            scoped_fs: &scoped_fs,
            parachain: None,
            bootnodes_addr: vec![],
        };

        let global_files_to_inject = vec![
            TransferedFile {
                local_path: PathBuf::from(format!("{}/rococo-local.json", ns.base_dir())),
                remote_path: PathBuf::from("/cfg/rococo-local.json"),
            }
        ];

        let spawning_tasks = bootnodes.iter().map(|node| self.spawn_node(node, global_files_to_inject.clone(), &ctx));
        let mut running_nodes: Vec<NetworkNode> = futures::future::try_join_all(spawning_tasks).await?;

        // calculate the bootnodes addr from the running nodes
        // TODO: we just use localhost for now
        let mut bootnodes_addr : Vec<String> = vec![];
        for node in running_nodes.iter() {
            bootnodes_addr.push(
                generators::bootnode_addr::generate(&node.spec.peer_id, &LOCALHOST, node.spec.p2p_port.0, &node.inner.args(), &node.spec.p2p_cert_hash)?
            );
        }

        ctx.bootnodes_addr = bootnodes_addr;

        let spawning_tasks = relaynodes.iter().map(|node| self.spawn_node(node, global_files_to_inject.clone(), &ctx));
        running_nodes.append(&mut futures::future::try_join_all(spawning_tasks).await?);

        // spawn the rest of the nodes (in batches)

        // add-ons (introspector/tracing/etc)

        // verify nodes (clean metrics cache?)

        // write zombie.json state file

        // return `Network` instance
        Ok(())
    }

    async fn spawn_node<'a>(&self, node: &NodeSpec, mut files_to_inject: Vec<TransferedFile>, ctx: &SpawnNodeCtx<'a, T>) -> Result<NetworkNode, ProviderError> {
        if node.is_validator {
            // Generate keystore for node
            let node_files_path = if let Some(para) = ctx.parachain {
                para.id.to_string()
            } else {
                node.name.clone()
            };
            let key_filenames = generators::keystore::generate_keystore(&node.accounts, &node_files_path, ctx.scoped_fs).await.unwrap();
            // Paths returned are relative to the base dir, we need to convert into
            // fullpaths to inject them in the nodes.
            for key_filename in key_filenames {
                let f = TransferedFile {
                    local_path: PathBuf::from(format!("{}/{}/{}", ctx.ns.base_dir(), node_files_path, key_filename.to_string_lossy())),
                    remote_path: PathBuf::from(format!("{}/chains/{}/keystore/{}", "/data", ctx.chain_id, key_filename.to_string_lossy()))
                };
                files_to_inject.push(f);
            }
        }

        let base_dir = format!("{}/{}", ctx.ns.base_dir(), &node.name);
        let cfg_path = format!("{}/cfg", &base_dir);
        let data_path = format!("{}/data", &base_dir);
        let relay_data_path = format!("{}/relay-data", ctx.ns.base_dir());
        let gen_opts = generators::command::GenCmdOptions {
            cfg_path: &cfg_path, // TODO: get from provider/ns
            data_path: &data_path, // TODO: get from provider
            relay_data_path: &relay_data_path, // TODO: get from provider
            use_wrapper: false, // TODO: get from provider
            bootnode_addr: ctx.bootnodes_addr.clone()
        };

        let (cmd, args) = match ctx.role {
            ZombieRole::Node => {
                generators::command::generate_for_node(&node, gen_opts, None)
            },
            _ => unreachable!()
            // ZombieRole::Bootnode => todo!(),
            // ZombieRole::Collator => todo!(),
            // ZombieRole::CumulusCollator => todo!(),
            // ZombieRole::Companion => todo!(),
        };

        println!("cmd: {:#?}", cmd);
        println!("args: {:#?}", args);

        let spawn_ops = SpawnNodeOptions {
            name: node.name.clone(),
            command: cmd,
            args,
            env: node.env.iter().map(|env| (env.name.clone(), env.value.clone())).collect(),
            injected_files: files_to_inject,
            created_paths: vec![PathBuf::from(format!("/data/chains/{}/keystore", ctx.chain_id))],
        };

        let running_node = ctx.ns.spawn_node(spawn_ops).await?;

        println!("🚀 {}, should be running now", node.name);
        Ok( NetworkNode {
            inner: running_node,
            spec: node.clone(),
            name: node.name.clone(),
            ws_uri: format!("ws://{}:{}", LOCALHOST, node.ws_port.0),
            prometheus_uri: format!("http://{}:{}/metrics", LOCALHOST, node.prometheus_port.0),
        })
    }

}

// TODO: get the fs from `DynNamespace` will make this not needed
// but the FileSystem trait isn't object-safe so we can't pass around
// as `dyn FileSystem`. We can refactor or using some `erase` techniques
// to resolve this and remove this struct
pub struct ScopedFilesystem<'a, FS: FileSystem> {
    fs: &'a FS,
    base_dir: &'a str
}

impl<'a, FS: FileSystem> ScopedFilesystem<'a, FS> {
    fn new(fs: &'a FS, base_dir: &'a str) -> Self { Self { fs, base_dir } }

    async fn copy_files(&self, files: Vec<&TransferedFile>) -> Result<(),FileSystemError> {
        for file in files {
            let full_remote_path = PathBuf::from(format!("{}/{}",self.base_dir, file.remote_path.to_string_lossy()));
            self.fs.copy(file.local_path.as_path(), full_remote_path).await?;
        }
        Ok(())
    }

    async fn read_to_string(&self, file: impl AsRef<Path>) -> Result<String, FileSystemError> {
        let full_path = PathBuf::from(format!("{}/{}",self.base_dir, file.as_ref().to_string_lossy()));
        let content  = self.fs.read_to_string(full_path).await?;
        Ok(content)
    }

    async fn create_dir(&self, path: impl AsRef<Path>) -> Result<(), FileSystemError> {
        let path = PathBuf::from(format!("{}/{}",self.base_dir, path.as_ref().to_string_lossy()));
        self.fs.create_dir(path).await.map_err(Into::into)
    }

    async fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), FileSystemError> {
        let path = PathBuf::from(format!("{}/{}",self.base_dir, path.as_ref().to_string_lossy()));
        self.fs.create_dir_all(path).await.map_err(Into::into)
    }

    async fn write(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]> + Send
    ) ->  Result<(), FileSystemError> {
        let path = PathBuf::from(format!("{}/{}",self.base_dir, path.as_ref().to_string_lossy()));
        self.fs.write(path, contents).await.map_err(Into::into)
    }
}


pub enum ZombieRole {
    Temp,
    Node,
    Bootnode,
    Collator,
    CumulusCollator,
    Companion,
}

struct SpawnNodeCtx<'a, T: FileSystem> {
    chain_id: &'a str,
    chain: &'a str,
    role: ZombieRole,
    ns: &'a DynNamespace,
    scoped_fs: &'a ScopedFilesystem<'a, T>,
    parachain: Option<&'a ParachainSpec>,
    /// The string represenation of the bootnode addres to pass to nodes
    bootnodes_addr: Vec<String>,
}

pub struct Network {
    ns: DynNamespace,
    relay: Relaychain,
    initial_spec: NetworkSpec
}

pub struct Relaychain {
    chain_id: String,
    chain_spec_path: PathBuf,
    nodes: Vec<NetworkNode>,
}

pub struct NetworkNode {
    inner: DynNode,
    // TODO: do we need the full spec here?
    // Maybe a reduce set of values.
    spec: NodeSpec,
    name: String,
    ws_uri: String,
    prometheus_uri: String,
}