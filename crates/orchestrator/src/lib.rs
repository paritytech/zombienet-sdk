// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code)]

mod errors;
mod generators;
mod network_spec;
mod shared;
mod spawner;

use std::{time::Duration, path::{PathBuf, Path}, collections::HashMap};

use configuration::{NetworkConfig, types::{RegistrationStrategy, Command, Arg, Image}, shared::node::EnvVar};
use errors::OrchestratorError;
use network_spec::{NetworkSpec, node::NodeSpec, parachain::ParachainSpec};
use provider::{Provider, types::{TransferedFile, SpawnNodeOptions, Port}, DynNamespace, DynNode, constants::LOCALHOST};
use shared::types::ChainDefaultContext;
use support::fs::{FileSystem, FileSystemError};
use tokio::time::timeout;

use crate::generators::chain_spec::ParaGenesisConfig;

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

    pub async fn spawn(&self, network_config: NetworkConfig) -> Result<Network<T>, OrchestratorError> {
        let global_timeout = network_config.global_settings().network_spawn_timeout();
        let network_spec = NetworkSpec::from_config(&network_config).await?;

        timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_spec),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout))?
    }

    async fn spawn_inner(&self, mut network_spec: NetworkSpec) -> Result<Network<T>, OrchestratorError> {
        // main driver for spawn the network
        println!("{:#?}", network_spec);

        // create namespace
        let ns = self.provider.create_namespace().await?;


        println!("{:#?}", ns.id());
        println!("{:#?}", ns.base_dir());

        // TODO: noop for native
        // Static setup
        // ns.static_setup().await?;

        let base_dir = ns.base_dir();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);
        // Create chain-spec for relaychain
        network_spec.relaychain.chain_spec.build(&ns, &scoped_fs).await?;

        // TODO: move to logger
        // println!("{:#?}", network_spec.relaychain.chain_spec);

        // Create parachain artifacts (chain-spec, wasm, state)
        let relay_chain_id = network_spec.relaychain.chain_spec.read_chain_id(&scoped_fs).await?;
        let relay_chain_name = network_spec.relaychain.chain.as_str();
        // TODO: if we don't need to register this para we can skip it
        for para in network_spec.parachains.iter_mut() {
            let para_cloned = para.clone();
            let chain_spec_raw_path = if let Some(chain_spec) = para.chain_spec.as_mut() {
                chain_spec.build(&ns, &scoped_fs).await?;
                // TODO: move to logger
                // println!("{:#?}", chain_spec);

                chain_spec.customize_para(&para_cloned, &relay_chain_id, &scoped_fs).await?;
                chain_spec.build_raw(&ns).await?;


                let chain_spec_raw_path = chain_spec.raw_path().ok_or(OrchestratorError::InvariantError("chain-spec raw path should be set now"))?;
                Some(chain_spec_raw_path)
            } else {
                None
            };

            // TODO: this need to be abstracted in a single call to generate_files.
            scoped_fs.create_dir(para.id.to_string()).await?;
            // create wasm/state
            para.genesis_state.build(chain_spec_raw_path, format!("{}/genesis-state", para.id),&ns, &scoped_fs).await?;
            para.genesis_wasm.build(chain_spec_raw_path, format!("{}/genesis-wasm", para.id),&ns, &scoped_fs).await?;

        }

        let para_to_register_in_genesis: Vec<&ParachainSpec> = network_spec.parachains.iter()
            .filter(|para| {
                match &para.registration_strategy {
                    RegistrationStrategy::InGenesis => true,
                    RegistrationStrategy::UsingExtrinsic => false,
                }
            }).collect();

        let mut para_artifacts = vec![];
        for para in para_to_register_in_genesis {
            let genesis_config = ParaGenesisConfig {
                state_path: para.genesis_state.artifact_path().ok_or(OrchestratorError::InvariantError("artifact path for state must be set at this point"))?,
                wasm_path: para.genesis_wasm.artifact_path().ok_or(OrchestratorError::InvariantError("artifact path for wasm must be set at this point"))?,
                id: para.id,
                as_parachain: para.onboard_as_parachain
            };
            para_artifacts.push(genesis_config)
        };


        // Customize relaychain
        network_spec.relaychain.chain_spec.customize_relay(&network_spec.relaychain, &network_spec.hrmp_channels, para_artifacts, &scoped_fs).await?;

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
        };

        let global_files_to_inject = vec![
            TransferedFile {
                local_path: PathBuf::from(format!("{}/{relay_chain_name}.json", ns.base_dir())),
                remote_path: PathBuf::from(format!("/cfg/{relay_chain_name}.json")),
            }
        ];

        let r = Relaychain::new(relay_chain_name.to_string(),PathBuf::from(network_spec.relaychain.chain_spec.raw_path().ok_or(OrchestratorError::InvariantError("chain-spec raw path should be set now"))?));
        let mut network = Network::new_with_relay(r, ns.clone(), self.filesystem.clone(), network_spec.clone());

        let spawning_tasks = bootnodes.iter_mut().map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));

        // Calculate the bootnodes addr from the running nodes
        let mut bootnodes_addr : Vec<String> = vec![];
        for node  in futures::future::try_join_all(spawning_tasks).await? {
            bootnodes_addr.push(
                // TODO: we just use localhost for now
                generators::bootnode_addr::generate(&node.spec.peer_id, &LOCALHOST, node.spec.p2p_port.0, &node.inner.args(), &node.spec.p2p_cert_hash)?
            );
            // Add the node to the `Network` instance
            network.add_running_node(node, None);
        }


        ctx.bootnodes_addr = &bootnodes_addr;

        // spawn the rest of the nodes (TODO: in batches)
        let spawning_tasks = relaynodes.iter().map(|node| spawner::spawn_node(node, global_files_to_inject.clone(), &ctx));
        for node  in futures::future::try_join_all(spawning_tasks).await? {
            // Add the node to the `Network` instance
            network.add_running_node(node, None);
        }

        // Add the bootnodes to the relaychain spec file
        network_spec.relaychain.chain_spec.add_bootnodes(&scoped_fs, &bootnodes_addr).await?;

        // spawn paras
        for para in network_spec.parachains.iter() {
            // parachain id is used for the keystore
            let parachain_id = if let Some(chain_spec) = para.chain_spec.as_ref() {
                let id = chain_spec.read_chain_id(&scoped_fs).await?;
                let raw_path = chain_spec.raw_path().ok_or(OrchestratorError::InvariantError("chain-spec path should be set by now."))?;
                let mut running_para = Parachain::with_chain_spec( para.id, raw_path);
                if let Some(chain_name) = chain_spec.chain_name() {
                    running_para.chain = Some(chain_name.to_string());
                }
                network.add_para(running_para);

                Some(id)
            } else {
                network.add_para(Parachain::new(para.id));

                None
            };

            let ctx_para  = SpawnNodeCtx {
                parachain: Some(para),
                parachain_id: parachain_id.as_deref(),
                role: if para.is_cumulus_based { ZombieRole::CumulusCollator } else { ZombieRole::Collator },
                bootnodes_addr: &vec![],
                ..ctx.clone()
            };
            let mut para_files_to_inject = global_files_to_inject.clone();
            if para.is_cumulus_based {
                para_files_to_inject.push(TransferedFile {
                    local_path: PathBuf::from(format!("{}/{}.json", ns.base_dir(), para.id)),
                    remote_path: PathBuf::from(format!("/cfg/{}.json", para.id)),
                });
            }

            let spawning_tasks = para.collators.iter().map(|node| spawner::spawn_node(node, para_files_to_inject.clone(), &ctx_para));
            // TODO: Add para to Network instance
            for node  in futures::future::try_join_all(spawning_tasks).await? {
                network.add_running_node(node, Some(para.id));
            }
        }

        // TODO:

        // - add-ons (introspector/tracing/etc)

        // - verify nodes (clean metrics cache?)

        // - write zombie.json state file (we should defined in a way we can load later)

        Ok(network)
    }

    // TODO: move this to other module
    async fn spawn_node<'a>(node: &NodeSpec, mut files_to_inject: Vec<TransferedFile>, ctx: &SpawnNodeCtx<'a, T>) -> Result<NetworkNode, OrchestratorError> {
        let mut created_paths = vec![];
        // Create and inject the keystore IFF
        // - The node is validator in the relaychain
        // - The node is collator (encoded as validator) and the parachain is cumulus_based
        // (parachain_id) should be set then.
        if node.is_validator && ( ctx.parachain.is_none() || ctx.parachain_id.is_some() ) {
            // Generate keystore for node
            let node_files_path = if let Some(para) = ctx.parachain {
                para.id.to_string()
            } else {
                node.name.clone()
            };
            let key_filenames = generators::keystore::generate_keystore(&node.accounts, &node_files_path, ctx.scoped_fs).await.unwrap();

            // Paths returned are relative to the base dir, we need to convert into
            // fullpaths to inject them in the nodes.
            let remote_keystore_chain_id = if let Some(id) = ctx.parachain_id {
                id
            } else {
                ctx.chain_id
            };

            for key_filename in key_filenames {
                let f = TransferedFile {
                    local_path: PathBuf::from(format!("{}/{}/{}", ctx.ns.base_dir(), node_files_path, key_filename.to_string_lossy())),
                    remote_path: PathBuf::from(format!("/data/chains/{}/keystore/{}", remote_keystore_chain_id, key_filename.to_string_lossy()))
                };
                files_to_inject.push(f);
            }
            created_paths.push(PathBuf::from(format!("/data/chains/{}/keystore",remote_keystore_chain_id )));
        }

        let base_dir = format!("{}/{}", ctx.ns.base_dir(), &node.name);
        let cfg_path = format!("{}/cfg", &base_dir);
        let data_path = format!("{}/data", &base_dir);
        let relay_data_path = format!("{}/relay-data", &base_dir);
        let gen_opts = generators::command::GenCmdOptions {
            relay_chain_name: ctx.chain,
            cfg_path: &cfg_path, // TODO: get from provider/ns
            data_path: &data_path, // TODO: get from provider
            relay_data_path: &relay_data_path, // TODO: get from provider
            use_wrapper: false, // TODO: get from provider
            bootnode_addr: ctx.bootnodes_addr.clone()
        };

        let (cmd, args) = match ctx.role {
            // Collator should be `non-cumulus` one (e.g adder/undying)
            ZombieRole::Node | ZombieRole::Collator => {

                let maybe_para_id = if let Some(para) = ctx.parachain {
                    Some(para.id)
                } else {
                    None
                };

                generators::command::generate_for_node(&node, gen_opts, maybe_para_id)
            },
            ZombieRole::CumulusCollator => {
                let para = ctx.parachain.expect("parachain must be part of the context, this is a bug".into());
                generators::command::generate_for_cumulus_node(&node, gen_opts, para.id)
            }
            _ => unreachable!()
            // TODO: do we need those?
            // ZombieRole::Bootnode => todo!(),
            // ZombieRole::Companion => todo!(),
        };

        println!("\n");
        println!("🚀 {}, spawning.... with command:", node.name);
        println!("{}", format!("{cmd} {}", args.join(" ")));

        let spawn_ops = SpawnNodeOptions {
            name: node.name.clone(),
            command: cmd,
            args,
            env: node.env.iter().map(|env| (env.name.clone(), env.value.clone())).collect(),
            injected_files: files_to_inject,
            created_paths,
        };

        // Drops the port parking listeners before spawn
        node.p2p_port.drop_listener();
        node.rpc_port.drop_listener();
        node.prometheus_port.drop_listener();

        let running_node = ctx.ns.spawn_node(spawn_ops).await?;

        let ws_uri = format!("ws://{}:{}", LOCALHOST, node.rpc_port.0);
        let prometheus_uri = format!("http://{}:{}/metrics", LOCALHOST, node.prometheus_port.0);
        println!("🚀 {}, should be running now", node.name);
        println!("🚀 {} : direct link https://polkadot.js.org/apps/?rpc={ws_uri}#/explorer", node.name);
        println!("🚀 {} : metrics link {prometheus_uri}", node.name);
        println!("\n");
        Ok( NetworkNode {
            inner: running_node,
            spec: node.clone(),
            name: node.name.clone(),
            ws_uri,
            prometheus_uri,
        })
    }

}

// TODO: get the fs from `DynNamespace` will make this not needed
// but the FileSystem trait isn't object-safe so we can't pass around
// as `dyn FileSystem`. We can refactor or using some `erase` techniques
// to resolve this and remove this struct
#[derive(Clone)]
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

#[derive(Clone)]
pub enum ZombieRole {
    Temp,
    Node,
    Bootnode,
    Collator,
    CumulusCollator,
    Companion,
}

#[derive(Clone)]
pub struct SpawnNodeCtx<'a, T: FileSystem> {
    // Relaychain id, from the chain-spec (e.g rococo_local_testnet)
    chain_id: &'a str,
    // Parachain id, from the chain-spec (e.g local_testnet)
    parachain_id: Option<&'a str>,
    // Relaychain chain name (e.g rococo-local)
    chain: &'a str,
    // Role of the node in the network
    role: ZombieRole,
    // Ref to the namespace
    ns: &'a DynNamespace,
    // Ref to an scoped filesystem (encapsulate fs actions inside the ns directory)
    scoped_fs: &'a ScopedFilesystem<'a, T>,
    // Ref to a parachain (used to spawn collators)
    parachain: Option<&'a ParachainSpec>,
    /// The string represenation of the bootnode addres to pass to nodes
    bootnodes_addr: &'a Vec<String>,
}


pub struct Network<T: FileSystem> {
    ns: DynNamespace,
    filesystem: T,
    relay: Relaychain,
    initial_spec: NetworkSpec,
    parachains: HashMap<u32, Parachain>,
    nodes_by_name: HashMap<String, NetworkNode>
}

impl<T: FileSystem> std::fmt::Debug for Network<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network").field("ns", &"ns_skipped").field("relay", &self.relay).field("initial_spec", &self.initial_spec).field("parachains", &self.parachains).field("nodes_by_name", &self.nodes_by_name).finish()
    }
}

#[derive(Default, Debug, Clone)]
pub struct AddNodeOpts {
    pub image: Option<Image>,
    pub command: Option<Command>,
    pub args: Vec<Arg>,
    pub env: Vec<EnvVar>,
    pub is_validator: bool,
    pub rpc_port: Option<Port>,
    pub prometheus_port: Option<Port>,
    pub p2p_port: Option<Port>,
}


impl<T: FileSystem> Network<T> {
    fn new_with_relay(relay: Relaychain, ns: DynNamespace, fs: T, initial_spec: NetworkSpec) -> Self {
        Self {
            ns,
            filesystem: fs,
            relay,
            initial_spec,
            parachains: Default::default(),
            nodes_by_name: Default::default(),
        }
    }

    // Pub API

    // Teardown the network
    // destroy()

    // Could be for relay/para?
    // pub fn add_node(&mut self, name: impl Into<String>, cmd: Command, args: Vec<Arg>, env: Vec<EnvVar>, is_validator: bool, para_id: Option<u32>) -> Result<(), anyhow::Error> {
    pub async fn add_node(&mut self, name: impl Into<String>, options: AddNodeOpts ) -> Result<(), anyhow::Error> {
        // build context
        let spec = &self.initial_spec.relaychain;
        let chain_context = ChainDefaultContext {
            default_command: spec.default_command.as_ref(),
            default_image: spec.default_image.as_ref(),
            default_resources: spec.default_resources.as_ref(),
            default_db_snapshot: spec.default_db_snapshot.as_ref(),
            default_args: spec.default_args.iter().collect(),
        };

        let node_spec = network_spec::node::NodeSpec::from_ad_hoc(name.into(), options, &chain_context)?;
        let base_dir = self.ns.base_dir();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);


        // TODO: we want to still supporting spawn a dedicated bootnode??
        let ctx = SpawnNodeCtx {
            chain_id: &self.relay.chain,
            parachain_id: None,
            chain: &self.relay.chain,
            role: ZombieRole::Node,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            parachain: None,
            bootnodes_addr: &vec![],
        };

        let global_files_to_inject = vec![
            TransferedFile {
                local_path: PathBuf::from(format!("{}/{}.json", self.ns.base_dir(), self.relay.chain)),
                remote_path: PathBuf::from(format!("/cfg/{}.json", self.relay.chain)),
            }
        ];

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;
        self.add_running_node(node, None);


        Ok(())
    }

    // This should include at least of collator?
    // add_parachain()

    // deregister and stop the collator?
    // remove_parachain()

    // Node actions
    pub async fn pause_node(&self, node_name: impl Into<String>) -> Result<(), anyhow::Error> {
        let node_name = node_name.into();
        if let Some(node) = self.nodes_by_name.get(&node_name) {
            node.inner.pause().await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("can't find the node!"))
        }
    }

    pub async fn resume_node(&self, node_name: impl Into<String>) -> Result<(), anyhow::Error> {
        let node_name = node_name.into();
        if let Some(node) = self.nodes_by_name.get(&node_name) {
            node.inner.resume().await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("can't find the node!"))
        }
    }

    pub async fn restart_node(&self, node_name: impl Into<String>, after: Option<Duration>) -> Result<(), anyhow::Error> {
        let node_name = node_name.into();
        if let Some(node) = self.nodes_by_name.get(&node_name) {
            node.inner.restart(after).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("can't find the node!"))
        }
    }


    // Internal API
    fn add_running_node(&mut self, node: NetworkNode, para_id: Option<u32>) {
        if let Some(para_id) = para_id {
            if let Some(para) = self.parachains.get_mut(&para_id) {
                para.collators.push(node.clone());
            } else {
                unreachable!()
            }
        } else {
            self.relay.nodes.push(node.clone());
        }
        // TODO: we should hold a ref to the node in the vec in the future.
        let node_name = node.name.clone();
        self.nodes_by_name.insert(node_name, node);
    }

    fn add_para(&mut self, para: Parachain) {
        self.parachains.insert(para.para_id, para);
    }

    fn id(&self) -> String {
        self.ns.id()
    }

    fn relaychain(&self) -> &Relaychain {
        &self.relay
    }

    fn parachain(&self, para_id: u32) -> Option<&Parachain> {
        self.parachains.get(&para_id)
    }

    fn parachains(&self) -> Vec<&Parachain> {
        self.parachains.values().collect()
    }

}

#[derive(Debug)]
pub struct Relaychain {
    chain: String,
    chain_spec_path: PathBuf,
    nodes: Vec<NetworkNode>,
}

impl Relaychain {
    fn new(chain: String, chain_spec_path: PathBuf) -> Self {
        Self {
            chain,
            chain_spec_path,
            nodes: Default::default()
        }
    }
}

#[derive(Debug)]
pub struct Parachain {
    chain: Option<String>,
    para_id: u32,
    chain_spec_path: Option<PathBuf>,
    collators: Vec<NetworkNode>,
}

impl Parachain {
    fn new(para_id: u32) -> Self {
        Self {
            chain: None,
            para_id,
            chain_spec_path: None,
            collators: Default::default()
        }
    }

    fn with_chain_spec(para_id: u32, chain_spec_path: impl AsRef<Path>) -> Self {
        Self {
            para_id,
            chain: None,
            chain_spec_path: Some(chain_spec_path.as_ref().into()),
            collators: Default::default()
        }
    }
}


#[derive(Clone)]
pub struct NetworkNode {
    inner: DynNode,
    // TODO: do we need the full spec here?
    // Maybe a reduce set of values.
    spec: NodeSpec,
    name: String,
    ws_uri: String,
    prometheus_uri: String,
}

impl NetworkNode {
    fn new(inner: DynNode, spec: NodeSpec, _ip: String) -> Self {
        let name = spec.name.clone();
        let ws_uri = "".into();
        let prometheus_uri = "".into();

        Self {
            inner,
            spec,
            name,
            ws_uri,
            prometheus_uri,
        }
    }
}

impl std::fmt::Debug for NetworkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkNode").field("inner", &"inner_skipped").field("spec", &self.spec).field("name", &self.name).field("ws_uri", &self.ws_uri).field("prometheus_uri", &self.prometheus_uri).finish()
    }
}