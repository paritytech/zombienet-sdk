pub mod chain_upgrade;
pub mod node;
pub mod parachain;
pub mod relaychain;

use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, RwLock},
    time::Duration,
};

use configuration::{
    para_states::{Initial, Running},
    shared::{helpers::generate_unique_node_name_from_names, node::EnvVar},
    types::{Arg, Command, Image, Port, ValidationContext},
    ParachainConfig, ParachainConfigBuilder, RegistrationStrategy,
};
use provider::{types::TransferedFile, DynNamespace, ProviderError};
use serde::Serialize;
use support::fs::FileSystem;
use tracing::{error, warn};

use self::{node::NetworkNode, parachain::Parachain, relaychain::Relaychain};
use crate::{
    generators::chain_spec::ChainSpec,
    network_spec::{self, NetworkSpec},
    shared::{
        constants::NODE_MONITORING_INTERVAL_SECONDS,
        macros,
        types::{ChainDefaultContext, RegisterParachainOptions},
    },
    spawner::{self, SpawnNodeCtx},
    ScopedFilesystem, ZombieRole,
};

#[derive(Serialize)]
pub struct Network<T: FileSystem> {
    #[serde(skip)]
    ns: DynNamespace,
    #[serde(skip)]
    filesystem: T,
    relay: Relaychain,
    initial_spec: NetworkSpec,
    parachains: HashMap<u32, Vec<Parachain>>,
    #[serde(skip)]
    nodes_by_name: HashMap<String, NetworkNode>,
    #[serde(skip)]
    nodes_to_watch: Arc<RwLock<Vec<NetworkNode>>>,
}

impl<T: FileSystem> std::fmt::Debug for Network<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network")
            .field("ns", &"ns_skipped")
            .field("relay", &self.relay)
            .field("initial_spec", &self.initial_spec)
            .field("parachains", &self.parachains)
            .field("nodes_by_name", &self.nodes_by_name)
            .finish()
    }
}

macros::create_add_options!(AddNodeOptions {
    chain_spec: Option<PathBuf>
});

macros::create_add_options!(AddCollatorOptions {
    chain_spec: Option<PathBuf>,
    chain_spec_relay: Option<PathBuf>
});

impl<T: FileSystem> Network<T> {
    pub(crate) fn new_with_relay(
        relay: Relaychain,
        ns: DynNamespace,
        fs: T,
        initial_spec: NetworkSpec,
    ) -> Self {
        Self {
            ns,
            filesystem: fs,
            relay,
            initial_spec,
            parachains: Default::default(),
            nodes_by_name: Default::default(),
            nodes_to_watch: Default::default(),
        }
    }

    // Pubic API
    pub fn ns_name(&self) -> String {
        self.ns.name().to_string()
    }

    pub fn base_dir(&self) -> Option<&str> {
        self.ns.base_dir().to_str()
    }

    pub fn relaychain(&self) -> &Relaychain {
        &self.relay
    }

    // Teardown the network
    pub async fn destroy(self) -> Result<(), ProviderError> {
        self.ns.destroy().await
    }

    /// Add a node to the relaychain
    // The new node is added to the running network instance.
    /// # Example:
    /// ```rust
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem};
    /// # use zombienet_orchestrator::{errors, AddNodeOptions, Orchestrator};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), errors::OrchestratorError> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    /// let mut network = orchestrator.spawn(config).await?;
    ///
    /// // Create the options to add the new node
    /// let opts = AddNodeOptions {
    ///     rpc_port: Some(9444),
    ///     is_validator: true,
    ///     ..Default::default()
    /// };
    ///
    /// network.add_node("new-node", opts).await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn add_node(
        &mut self,
        name: impl Into<String>,
        options: AddNodeOptions,
    ) -> Result<(), anyhow::Error> {
        let name = generate_unique_node_name_from_names(
            name,
            &mut self.nodes_by_name.keys().cloned().collect(),
        );

        let relaychain = self.relaychain();

        let chain_spec_path = if let Some(chain_spec_custom_path) = &options.chain_spec {
            chain_spec_custom_path.clone()
        } else {
            PathBuf::from(format!(
                "{}/{}.json",
                self.ns.base_dir().to_string_lossy(),
                relaychain.chain
            ))
        };

        let chain_context = ChainDefaultContext {
            default_command: self.initial_spec.relaychain.default_command.as_ref(),
            default_image: self.initial_spec.relaychain.default_image.as_ref(),
            default_resources: self.initial_spec.relaychain.default_resources.as_ref(),
            default_db_snapshot: self.initial_spec.relaychain.default_db_snapshot.as_ref(),
            default_args: self.initial_spec.relaychain.default_args.iter().collect(),
        };

        let mut node_spec = network_spec::node::NodeSpec::from_ad_hoc(
            &name,
            options.into(),
            &chain_context,
            false,
        )?;

        node_spec.available_args_output = Some(
            self.initial_spec
                .node_available_args_output(&node_spec, self.ns.clone())
                .await?,
        );

        let base_dir = self.ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        let ctx = SpawnNodeCtx {
            chain_id: &relaychain.chain_id,
            parachain_id: None,
            chain: &relaychain.chain,
            role: ZombieRole::Node,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            parachain: None,
            bootnodes_addr: &vec![],
            wait_ready: true,
            nodes_by_name: serde_json::to_value(&self.nodes_by_name)?,
        };

        let global_files_to_inject = vec![TransferedFile::new(
            chain_spec_path,
            PathBuf::from(format!("/cfg/{}.json", relaychain.chain)),
        )];

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;

        // TODO: register the new node as validator in the relaychain
        // STEPS:
        //  - check balance of `stash` derivation for validator account
        //  - call rotate_keys on the new validator
        //  - call setKeys on the new validator
        // if node_spec.is_validator {
        //     let running_node = self.relay.nodes.first().unwrap();
        //     // tx_helper::validator_actions::register(vec![&node], &running_node.ws_uri, None).await?;
        // }

        // Let's make sure node is up before adding
        node.wait_until_is_up(self.initial_spec.global_settings.network_spawn_timeout())
            .await?;

        // Add node to relaychain data
        self.add_running_node(node.clone(), None);

        Ok(())
    }

    /// Add a new collator to a parachain
    ///
    /// NOTE: if more parachains with given id available (rare corner case)
    /// then it adds collator to the first parachain
    ///
    /// # Example:
    /// ```rust
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem};
    /// # use zombienet_orchestrator::{errors, AddCollatorOptions, Orchestrator};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), anyhow::Error> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    /// let mut network = orchestrator.spawn(config).await?;
    ///
    /// let col_opts = AddCollatorOptions {
    ///     command: Some("polkadot-parachain".try_into()?),
    ///     ..Default::default()
    /// };
    ///
    /// network.add_collator("new-col-1", col_opts, 100).await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn add_collator(
        &mut self,
        name: impl Into<String>,
        options: AddCollatorOptions,
        para_id: u32,
    ) -> Result<(), anyhow::Error> {
        let name = generate_unique_node_name_from_names(
            name,
            &mut self.nodes_by_name.keys().cloned().collect(),
        );
        let spec = self
            .initial_spec
            .parachains
            .iter()
            .find(|para| para.id == para_id)
            .ok_or(anyhow::anyhow!(format!("parachain: {para_id} not found!")))?;
        let role = if spec.is_cumulus_based {
            ZombieRole::CumulusCollator
        } else {
            ZombieRole::Collator
        };
        let chain_context = ChainDefaultContext {
            default_command: spec.default_command.as_ref(),
            default_image: spec.default_image.as_ref(),
            default_resources: spec.default_resources.as_ref(),
            default_db_snapshot: spec.default_db_snapshot.as_ref(),
            default_args: spec.default_args.iter().collect(),
        };

        let parachain = self
            .parachains
            .get_mut(&para_id)
            .ok_or(anyhow::anyhow!(format!("parachain: {para_id} not found!")))?
            .get_mut(0)
            .ok_or(anyhow::anyhow!(format!("parachain: {para_id} not found!")))?;

        let base_dir = self.ns.base_dir().to_string_lossy();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        // TODO: we want to still supporting spawn a dedicated bootnode??
        let ctx = SpawnNodeCtx {
            chain_id: &self.relay.chain_id,
            parachain_id: parachain.chain_id.as_deref(),
            chain: &self.relay.chain,
            role,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            parachain: Some(spec),
            bootnodes_addr: &vec![],
            wait_ready: true,
            nodes_by_name: serde_json::to_value(&self.nodes_by_name)?,
        };

        let relaychain_spec_path = if let Some(chain_spec_custom_path) = &options.chain_spec_relay {
            chain_spec_custom_path.clone()
        } else {
            PathBuf::from(format!(
                "{}/{}.json",
                self.ns.base_dir().to_string_lossy(),
                self.relay.chain
            ))
        };

        let mut global_files_to_inject = vec![TransferedFile::new(
            relaychain_spec_path,
            PathBuf::from(format!("/cfg/{}.json", self.relay.chain)),
        )];

        let para_chain_spec_local_path = if let Some(para_chain_spec_custom) = &options.chain_spec {
            Some(para_chain_spec_custom.clone())
        } else if let Some(para_spec_path) = &parachain.chain_spec_path {
            Some(PathBuf::from(format!(
                "{}/{}",
                self.ns.base_dir().to_string_lossy(),
                para_spec_path.to_string_lossy()
            )))
        } else {
            None
        };

        if let Some(para_spec_path) = para_chain_spec_local_path {
            global_files_to_inject.push(TransferedFile::new(
                para_spec_path,
                PathBuf::from(format!("/cfg/{para_id}.json")),
            ));
        }

        let mut node_spec =
            network_spec::node::NodeSpec::from_ad_hoc(name, options.into(), &chain_context, true)?;

        node_spec.available_args_output = Some(
            self.initial_spec
                .node_available_args_output(&node_spec, self.ns.clone())
                .await?,
        );

        let node = spawner::spawn_node(&node_spec, global_files_to_inject, &ctx).await?;

        // Let's make sure node is up before adding
        node.wait_until_is_up(self.initial_spec.global_settings.network_spawn_timeout())
            .await?;

        parachain.collators.push(node.clone());
        self.add_running_node(node, None);

        Ok(())
    }

    /// Get a parachain config builder from a running network
    ///
    /// This allow you to build a new parachain config to be deployed into
    /// the running network.
    pub fn para_config_builder(&self) -> ParachainConfigBuilder<Initial, Running> {
        let used_ports = self
            .nodes_iter()
            .map(|node| node.spec())
            .flat_map(|spec| {
                [
                    spec.ws_port.0,
                    spec.rpc_port.0,
                    spec.prometheus_port.0,
                    spec.p2p_port.0,
                ]
            })
            .collect();

        let used_nodes_names = self.nodes_by_name.keys().cloned().collect();

        // need to inverse logic of generate_unique_para_id
        let used_para_ids = self
            .parachains
            .iter()
            .map(|(id, paras)| (*id, paras.len().saturating_sub(1) as u8))
            .collect();

        let context = ValidationContext {
            used_ports,
            used_nodes_names,
            used_para_ids,
        };
        let context = Rc::new(RefCell::new(context));

        ParachainConfigBuilder::new_with_running(context)
    }

    /// Add a new parachain to the running network
    ///
    /// # Arguments
    /// * `para_config` - Parachain configuration to deploy
    /// * `custom_relaychain_spec` - Optional path to a custom relaychain spec to use
    /// * `custom_parchain_fs_prefix` - Optional prefix to use when artifacts are created
    ///
    ///
    /// # Example:
    /// ```rust
    /// # use anyhow::anyhow;
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem};
    /// # use zombienet_orchestrator::{errors, AddCollatorOptions, Orchestrator};
    /// # use configuration::NetworkConfig;
    /// # async fn example() -> Result<(), anyhow::Error> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfig::load_from_toml("config.toml")?;
    /// let mut network = orchestrator.spawn(config).await?;
    /// let para_config = network
    ///     .para_config_builder()
    ///     .with_id(100)
    ///     .with_default_command("polkadot-parachain")
    ///     .with_collator(|c| c.with_name("col-100-1"))
    ///     .build()
    ///     .map_err(|_e| anyhow!("Building config"))?;
    ///
    /// network.add_parachain(&para_config, None, None).await?;
    ///
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn add_parachain(
        &mut self,
        para_config: &ParachainConfig,
        custom_relaychain_spec: Option<PathBuf>,
        custom_parchain_fs_prefix: Option<String>,
    ) -> Result<(), anyhow::Error> {
        // build
        let mut para_spec = network_spec::parachain::ParachainSpec::from_config(para_config)?;
        let base_dir = self.ns.base_dir().to_string_lossy().to_string();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);

        let mut global_files_to_inject = vec![];

        // get relaychain id
        let relay_chain_id = if let Some(custom_path) = custom_relaychain_spec {
            // use this file as relaychain spec
            global_files_to_inject.push(TransferedFile::new(
                custom_path.clone(),
                PathBuf::from(format!("/cfg/{}.json", self.relaychain().chain)),
            ));
            let content = std::fs::read_to_string(custom_path)?;
            ChainSpec::chain_id_from_spec(&content)?
        } else {
            global_files_to_inject.push(TransferedFile::new(
                PathBuf::from(format!(
                    "{}/{}",
                    scoped_fs.base_dir,
                    self.relaychain().chain_spec_path.to_string_lossy()
                )),
                PathBuf::from(format!("/cfg/{}.json", self.relaychain().chain)),
            ));
            self.relay.chain_id.clone()
        };

        let chain_spec_raw_path = para_spec
            .build_chain_spec(&relay_chain_id, &self.ns, &scoped_fs)
            .await?;

        // Para artifacts
        let para_path_prefix = if let Some(custom_prefix) = custom_parchain_fs_prefix {
            custom_prefix
        } else {
            para_spec.id.to_string()
        };

        scoped_fs.create_dir(&para_path_prefix).await?;
        // create wasm/state
        para_spec
            .genesis_state
            .build(
                chain_spec_raw_path.as_ref(),
                format!("{}/genesis-state", &para_path_prefix),
                &self.ns,
                &scoped_fs,
            )
            .await?;
        para_spec
            .genesis_wasm
            .build(
                chain_spec_raw_path.as_ref(),
                format!("{}/para_spec-wasm", &para_path_prefix),
                &self.ns,
                &scoped_fs,
            )
            .await?;

        let parachain =
            Parachain::from_spec(&para_spec, &global_files_to_inject, &scoped_fs).await?;
        let parachain_id = parachain.chain_id.clone();

        // Create `ctx` for spawn the nodes
        let ctx_para = SpawnNodeCtx {
            parachain: Some(&para_spec),
            parachain_id: parachain_id.as_deref(),
            role: if para_spec.is_cumulus_based {
                ZombieRole::CumulusCollator
            } else {
                ZombieRole::Collator
            },
            bootnodes_addr: &vec![],
            chain_id: &self.relaychain().chain_id,
            chain: &self.relaychain().chain,
            ns: &self.ns,
            scoped_fs: &scoped_fs,
            wait_ready: false,
            nodes_by_name: serde_json::to_value(&self.nodes_by_name)?,
        };

        // Register the parachain to the running network
        let first_node_url = self
            .relaychain()
            .nodes
            .first()
            .ok_or(anyhow::anyhow!(
                "At least one node of the relaychain should be running"
            ))?
            .ws_uri();

        if para_config.registration_strategy() == Some(&RegistrationStrategy::UsingExtrinsic) {
            let register_para_options = RegisterParachainOptions {
                id: parachain.para_id,
                // This needs to resolve correctly
                wasm_path: para_spec
                    .genesis_wasm
                    .artifact_path()
                    .ok_or(anyhow::anyhow!(
                        "artifact path for wasm must be set at this point",
                    ))?
                    .to_path_buf(),
                state_path: para_spec
                    .genesis_state
                    .artifact_path()
                    .ok_or(anyhow::anyhow!(
                        "artifact path for state must be set at this point",
                    ))?
                    .to_path_buf(),
                node_ws_url: first_node_url.to_string(),
                onboard_as_para: para_spec.onboard_as_parachain,
                seed: None, // TODO: Seed is passed by?
                finalization: false,
            };

            Parachain::register(register_para_options, &scoped_fs).await?;
        }

        // Spawn the nodes
        let spawning_tasks = para_spec
            .collators
            .iter()
            .map(|node| spawner::spawn_node(node, parachain.files_to_inject.clone(), &ctx_para));

        let running_nodes = futures::future::try_join_all(spawning_tasks).await?;

        // Let's make sure nodes are up before adding them
        let waiting_tasks = running_nodes.iter().map(|node| {
            node.wait_until_is_up(self.initial_spec.global_settings.network_spawn_timeout())
        });

        let _ = futures::future::try_join_all(waiting_tasks).await?;

        let running_para_id = parachain.para_id;
        self.add_para(parachain);
        for node in running_nodes {
            self.add_running_node(node, Some(running_para_id));
        }

        Ok(())
    }

    /// Register a parachain, which has already been added to the network (with manual registration
    /// strategy)
    ///
    /// # Arguments
    /// * `para_id` - Parachain Id
    ///
    ///
    /// # Example:
    /// ```rust
    /// # use anyhow::anyhow;
    /// # use provider::NativeProvider;
    /// # use support::{fs::local::LocalFileSystem};
    /// # use zombienet_orchestrator::Orchestrator;
    /// # use configuration::{NetworkConfig, NetworkConfigBuilder, RegistrationStrategy};
    /// # async fn example() -> Result<(), anyhow::Error> {
    /// #   let provider = NativeProvider::new(LocalFileSystem {});
    /// #   let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
    /// #   let config = NetworkConfigBuilder::new()
    /// #     .with_relaychain(|r| {
    /// #       r.with_chain("rococo-local")
    /// #         .with_default_command("polkadot")
    /// #         .with_node(|node| node.with_name("alice"))
    /// #     })
    /// #     .with_parachain(|p| {
    /// #       p.with_id(100)
    /// #         .with_registration_strategy(RegistrationStrategy::Manual)
    /// #         .with_default_command("test-parachain")
    /// #         .with_collator(|n| n.with_name("dave").validator(false))
    /// #     })
    /// #     .build()
    /// #     .map_err(|_e| anyhow!("Building config"))?;
    /// let mut network = orchestrator.spawn(config).await?;
    ///
    /// network.register_parachain(100).await?;
    ///
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn register_parachain(&mut self, para_id: u32) -> Result<(), anyhow::Error> {
        let para = self
            .initial_spec
            .parachains
            .iter()
            .find(|p| p.id == para_id)
            .ok_or(anyhow::anyhow!(
                "no parachain with id = {para_id} available",
            ))?;
        let para_genesis_config = para.get_genesis_config()?;
        let first_node_url = self
            .relaychain()
            .nodes
            .first()
            .ok_or(anyhow::anyhow!(
                "At least one node of the relaychain should be running"
            ))?
            .ws_uri();
        let register_para_options: RegisterParachainOptions = RegisterParachainOptions {
            id: para_id,
            // This needs to resolve correctly
            wasm_path: para_genesis_config.wasm_path.clone(),
            state_path: para_genesis_config.state_path.clone(),
            node_ws_url: first_node_url.to_string(),
            onboard_as_para: para_genesis_config.as_parachain,
            seed: None, // TODO: Seed is passed by?
            finalization: false,
        };
        let base_dir = self.ns.base_dir().to_string_lossy().to_string();
        let scoped_fs = ScopedFilesystem::new(&self.filesystem, &base_dir);
        Parachain::register(register_para_options, &scoped_fs).await?;

        Ok(())
    }

    // deregister and stop the collator?
    // remove_parachain()

    pub fn get_node(&self, name: impl Into<String>) -> Result<&NetworkNode, anyhow::Error> {
        let name = name.into();
        if let Some(node) = self.nodes_iter().find(|&n| n.name == name) {
            return Ok(node);
        }

        let list = self
            .nodes_iter()
            .map(|n| &n.name)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        Err(anyhow::anyhow!(
            "can't find node with name: {name:?}, should be one of {list}"
        ))
    }

    pub fn get_node_mut(
        &mut self,
        name: impl Into<String>,
    ) -> Result<&mut NetworkNode, anyhow::Error> {
        let name = name.into();
        self.nodes_iter_mut()
            .find(|n| n.name == name)
            .ok_or(anyhow::anyhow!("can't find node with name: {name:?}"))
    }

    pub fn nodes(&self) -> Vec<&NetworkNode> {
        self.nodes_by_name.values().collect::<Vec<&NetworkNode>>()
    }

    pub async fn detach(&self) {
        self.ns.detach().await
    }

    // Internal API
    pub(crate) fn add_running_node(&mut self, node: NetworkNode, para_id: Option<u32>) {
        if let Some(para_id) = para_id {
            if let Some(para) = self.parachains.get_mut(&para_id).and_then(|p| p.get_mut(0)) {
                para.collators.push(node.clone());
            } else {
                // is the first node of the para, let create the entry
                unreachable!()
            }
        } else {
            self.relay.nodes.push(node.clone());
        }
        // TODO: we should hold a ref to the node in the vec in the future.
        let node_name = node.name.clone();
        self.nodes_by_name.insert(node_name, node.clone());
        self.nodes_to_watch.write().unwrap().push(node.clone());
        node.mark_running();
    }

    pub(crate) fn add_para(&mut self, para: Parachain) {
        self.parachains.entry(para.para_id).or_default().push(para);
    }

    pub fn name(&self) -> &str {
        self.ns.name()
    }

    /// Get a first parachain from the list of the parachains with specified id.
    /// NOTE!
    /// Usually the list will contain only one parachain.
    /// Multiple parachains with the same id is a corner case.
    /// If this is the case then one can get such parachain with
    /// `parachain_by_unique_id()` method
    ///
    /// # Arguments
    /// * `para_id` - Parachain Id
    pub fn parachain(&self, para_id: u32) -> Option<&Parachain> {
        self.parachains.get(&para_id)?.first()
    }

    /// Get a parachain by its unique id.
    ///
    /// This is particularly useful if there are multiple parachains
    /// with the same id (this is a rare corner case).
    ///
    /// # Arguments
    /// * `unique_id` - unique id of the parachain
    pub fn parachain_by_unique_id(&self, unique_id: impl AsRef<str>) -> Option<&Parachain> {
        self.parachains
            .values()
            .flat_map(|p| p.iter())
            .find(|p| p.unique_id == unique_id.as_ref())
    }

    pub fn parachains(&self) -> Vec<&Parachain> {
        self.parachains.values().flatten().collect()
    }

    pub(crate) fn nodes_iter(&self) -> impl Iterator<Item = &NetworkNode> {
        self.relay.nodes.iter().chain(
            self.parachains
                .values()
                .flat_map(|p| p.iter())
                .flat_map(|p| &p.collators),
        )
    }

    pub(crate) fn nodes_iter_mut(&mut self) -> impl Iterator<Item = &mut NetworkNode> {
        self.relay.nodes.iter_mut().chain(
            self.parachains
                .values_mut()
                .flat_map(|p| p.iter_mut())
                .flat_map(|p| &mut p.collators),
        )
    }

    /// Waits given number of seconds until all nodes in the network report that they are
    /// up and running.
    ///
    /// # Arguments
    /// * `timeout_secs` - The number of seconds to wait.
    ///
    /// # Returns
    /// * `Ok()` if the node is up before timeout occured.
    /// * `Err(e)` if timeout or other error occurred while waiting.
    pub async fn wait_until_is_up(&self, timeout_secs: u64) -> Result<(), anyhow::Error> {
        let handles = self
            .nodes_iter()
            .map(|node| node.wait_until_is_up(timeout_secs));

        futures::future::try_join_all(handles).await?;

        Ok(())
    }

    pub(crate) async fn spawn_watching_task(&self) {
        let nodes_to_watch = Arc::clone(&self.nodes_to_watch);
        let ns = Arc::clone(&self.ns);
        let spawn_timeout = self.initial_spec.global_settings.node_spawn_timeout();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(NODE_MONITORING_INTERVAL_SECONDS)).await;

                let nodes = {
                    let guard = nodes_to_watch.read().unwrap();
                    guard
                        .iter()
                        .filter(|n| n.is_running())
                        .cloned()
                        .collect::<Vec<_>>()
                };

                let all_running = futures::future::try_join_all(
                    nodes.iter().map(|n| n.wait_until_is_up(spawn_timeout)),
                )
                .await;

                if let Err(e) = all_running {
                    warn!("detected unresponsive node: {e}. tearing the network down...");

                    if let Err(e) = ns.destroy().await {
                        error!("an error occurred during network teardown: {}", e);
                    }

                    std::process::exit(1);
                }
            }
        });
    }
}
