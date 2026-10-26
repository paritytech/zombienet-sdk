use std::{cell::RefCell, error::Error, fmt::Debug, marker::PhantomData, rc::Rc};

use serde::{Deserialize, Serialize};
use support::constants::{DEFAULT_TYPESTATE, THIS_IS_A_BUG};

use crate::{
    jam_genesis::{JamGenesisConfig, JamGenesisConfigBuilder},
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{merge_errors, merge_errors_vecs},
        macros::states,
        node::{self, JamNodeConfig, JamNodeConfigBuilder},
        resources::{Resources, ResourcesBuilder},
        types::{
            Arg, AssetLocation, Chain, ChainDefaultContext, Command, Image, ValidationContext,
        },
    },
    types::{JamNodeMode, JamProtocolParameterType},
    utils::{default_chain_jam, default_command_jam, default_protocol_para_type_jam},
};
/// A JAM chain configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JamchainConfig {
    /// Protocol params type to use, default tiny (max_validators: 6)
    #[serde(default = "default_protocol_para_type_jam")]
    protocol_params_type: JamProtocolParameterType,
    /// Id to use in config, default is `dev`
    #[serde(default = "default_chain_jam")]
    id: Chain,
    /// Default command to use for nodes.
    #[serde(default = "default_command_jam")]
    default_command: Option<Command>,
    /// Default image to use for nodes.
    default_image: Option<Image>,
    /// Default resources to use per pod.
    default_resources: Option<Resources>,
    /// Default arguments to pass to process (nodes).
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    default_args: Vec<Arg>,
    /// A pre-built JIP-4 chain spec, used as given (location can be url or file path).
    /// Without it the spec is built in process, customized by `genesis`.
    chain_spec_path: Option<AssetLocation>,
    /// Removed: the spec is built in process or given with `chain_spec_path`. Only read, to
    /// refuse configs that still set it.
    #[serde(default, skip_serializing)]
    chain_spec_command: Option<serde_json::Value>,
    /// Command to spawn corevm monitor (if present)
    corevm_monitor_command: Option<Command>,
    /// Command to spawn corevm builder (if present)
    corevm_builder_command: Option<Command>,
    /// Removed: use `genesis`. Only read, to refuse configs that still set it.
    #[serde(default, skip_serializing)]
    genesis_overrides: Option<serde_json::Value>,
    /// Customizations of the genesis built in process (unless `chain_spec_path` is set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    genesis: Option<JamGenesisConfig>,
    /// A list of nodes to run in this chain.
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    nodes: Vec<JamNodeConfig>,
}

impl JamchainConfig {
    /// The chain name.
    pub fn id(&self) -> &Chain {
        &self.id
    }

    /// The protocol parameters types used (default tiny)
    pub fn protocol_params_type(&self) -> &JamProtocolParameterType {
        &self.protocol_params_type
    }

    /// The default command used for nodes.
    pub fn default_command(&self) -> Option<&Command> {
        self.default_command.as_ref()
    }

    /// The default container image used for nodes.
    pub fn default_image(&self) -> Option<&Image> {
        self.default_image.as_ref()
    }

    /// The default resources limits used for nodes.
    pub fn default_resources(&self) -> Option<&Resources> {
        self.default_resources.as_ref()
    }

    /// The default arguments that will be used to launch the node command.
    pub fn default_args(&self) -> Vec<&Arg> {
        self.default_args.iter().collect::<Vec<&Arg>>()
    }

    /// The location of a pre-built chain spec, used instead of building one in process.
    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    /// The corevm monitor command used (if set).
    pub fn corevm_monitor_command(&self) -> Option<&Command> {
        self.corevm_monitor_command.as_ref()
    }

    /// The corevm builder command used (if set).
    pub fn corevm_builder_command(&self) -> Option<&Command> {
        self.corevm_builder_command.as_ref()
    }

    /// The customizations of the genesis built in process (if set).
    pub fn genesis(&self) -> Option<&JamGenesisConfig> {
        self.genesis.as_ref()
    }

    /// The nodes of the relay chain.
    pub fn nodes(&self) -> Vec<&JamNodeConfig> {
        self.nodes.iter().collect::<Vec<&JamNodeConfig>>()
    }

    /// Check how the spec is produced: a pre-built spec (`chain_spec_path`) or the built-in
    /// builder (the default, customized by `genesis`), and refuse the fields that used to pick
    /// the node's `gen-spec`.
    pub(crate) fn spec_source_errors(&self) -> Vec<anyhow::Error> {
        let mut errors = vec![];
        if self.chain_spec_command.is_some() {
            errors.push(anyhow::anyhow!(
                "chain_spec_command: removed, the JAM spec is built in process (customize it \
                 with `[jamchain.genesis]`) or given with `chain_spec_path`"
            ));
        }
        if self.genesis_overrides.is_some() {
            errors.push(anyhow::anyhow!(
                "genesis_overrides: removed, use `[jamchain.genesis]` (services, auth_queues, \
                 privileges, config_file)"
            ));
        }
        if self.genesis.is_some() && self.chain_spec_path.is_some() {
            errors.push(anyhow::anyhow!(
                "genesis: can't be used with `chain_spec_path`, the spec is used as given"
            ));
        }

        errors
    }

    /// Resolve the relative paths of the genesis against the network definition's directory.
    pub(crate) fn resolve_relative_paths(&mut self, base_dir: &std::path::Path) {
        if let Some(genesis) = self.genesis.as_mut() {
            genesis.resolve_relative_paths(base_dir);
        }
    }
}

states! {
    Initial,
    WithId,
    WithAtLeastOneNode
}

/// A relay chain configuration builder, used to build a [`JamchainConfig`] declaratively with fields validation.
pub struct JamchainConfigBuilder<State> {
    config: JamchainConfig,
    validation_context: Rc<RefCell<ValidationContext>>,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<State>,
}

impl Default for JamchainConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: JamchainConfig {
                protocol_params_type: JamProtocolParameterType::Tiny,
                id: "dev"
                    .try_into()
                    .expect(&format!("{DEFAULT_TYPESTATE} {THIS_IS_A_BUG}")),
                default_command: Some(
                    "polkajam"
                        .try_into()
                        .expect(&format!("{DEFAULT_TYPESTATE} {THIS_IS_A_BUG}")),
                ),
                default_image: None,
                default_resources: None,
                default_args: vec![],
                chain_spec_path: None,
                chain_spec_command: None,
                corevm_monitor_command: None,
                corevm_builder_command: None,
                genesis_overrides: None,
                genesis: None,
                nodes: vec![],
            },
            validation_context: Default::default(),
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> JamchainConfigBuilder<A> {
    fn transition<B>(
        config: JamchainConfig,
        validation_context: Rc<RefCell<ValidationContext>>,
        errors: Vec<anyhow::Error>,
    ) -> JamchainConfigBuilder<B> {
        JamchainConfigBuilder {
            config,
            validation_context,
            errors,
            _state: PhantomData,
        }
    }

    fn default_chain_context(&self) -> ChainDefaultContext {
        ChainDefaultContext {
            default_command: self.config.default_command.clone(),
            default_image: self.config.default_image.clone(),
            default_resources: self.config.default_resources.clone(),
            default_db_snapshot: None,
            default_args: self.config.default_args.clone(),
        }
    }

    fn create_node_builder<F>(&self, f: F) -> JamNodeConfigBuilder<node::Buildable>
    where
        F: FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    {
        f(JamNodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
    }

    /// Customize the genesis built in process, using a nested [`JamGenesisConfigBuilder`].
    ///
    /// Building the spec in process is the default, this is only needed to change what goes
    /// into genesis (services, auth queues, privileges, the bootstrap service...).
    pub fn with_genesis(
        self,
        f: impl FnOnce(JamGenesisConfigBuilder) -> JamGenesisConfigBuilder,
    ) -> Self {
        let builder = match self.config.genesis.clone() {
            Some(genesis) => JamGenesisConfigBuilder::from_config(genesis),
            None => JamGenesisConfigBuilder::new(),
        };
        match f(builder).build() {
            Ok(genesis) => Self::transition(
                JamchainConfig {
                    genesis: Some(genesis),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| anyhow::anyhow!("genesis.{error}"))
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Use this bootstrap service code (a path or a url) instead of the one embedded in
    /// zombienet. Shorthand for `with_genesis(|g| g.with_bootstrap_service(location))`.
    pub fn with_bootstrap_service(self, location: impl Into<AssetLocation>) -> Self {
        let location = location.into();
        self.with_genesis(|genesis| genesis.with_bootstrap_service(location))
    }
}

impl JamchainConfigBuilder<Initial> {
    pub fn new(
        validation_context: Rc<RefCell<ValidationContext>>,
    ) -> JamchainConfigBuilder<Initial> {
        Self {
            validation_context,
            ..Self::default()
        }
    }

    /// Set the id to use (e.g. dev).
    pub fn with_id<T>(self, chain: T) -> JamchainConfigBuilder<WithId>
    where
        T: TryInto<Chain>,
        T::Error: Error + Send + Sync + 'static,
    {
        match chain.try_into() {
            Ok(id) => Self::transition(
                JamchainConfig { id, ..self.config },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(self.errors, FieldError::Chain(error.into()).into()),
            ),
        }
    }
}

impl JamchainConfigBuilder<WithId> {
    /// Set the default command used for nodes. Can be overridden.
    pub fn with_default_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                JamchainConfig {
                    default_command: Some(command),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(self.errors, FieldError::DefaultCommand(error.into()).into()),
            ),
        }
    }

    /// Set the default container image used for nodes. Can be overridden.
    pub fn with_default_image<T>(self, image: T) -> Self
    where
        T: TryInto<Image>,
        T::Error: Error + Send + Sync + 'static,
    {
        match image.try_into() {
            Ok(image) => Self::transition(
                JamchainConfig {
                    default_image: Some(image),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(self.errors, FieldError::DefaultImage(error.into()).into()),
            ),
        }
    }

    /// Set the default resources limits used for nodes. Can be overridden.
    pub fn with_default_resources(
        self,
        f: impl FnOnce(ResourcesBuilder) -> ResourcesBuilder,
    ) -> Self {
        match f(ResourcesBuilder::new()).build() {
            Ok(default_resources) => Self::transition(
                JamchainConfig {
                    default_resources: Some(default_resources),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| FieldError::DefaultResources(error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Set the default arguments that will be used to execute the node command. Can be overridden.
    pub fn with_default_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            JamchainConfig {
                default_args: args,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Use a pre-built JIP-4 chain spec, instead of building it in process (the default).
    pub fn with_chain_spec_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            JamchainConfig {
                chain_spec_path: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the command used for corevm monitor.
    pub fn with_corevm_monitor_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                JamchainConfig {
                    corevm_monitor_command: Some(command),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(self.errors, FieldError::DefaultCommand(error.into()).into()),
            ),
        }
    }

    /// Set the command used for corevm builder.
    pub fn with_corevm_builder_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                JamchainConfig {
                    corevm_builder_command: Some(command),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(self.errors, FieldError::DefaultCommand(error.into()).into()),
            ),
        }
    }

    /// Add a new validator node using a nested [`JamNodeConfigBuilder`].
    /// The node will be configured as a validator (authority).
    pub fn with_validator(
        self,
        f: impl FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
        match self
            .create_node_builder(f)
            .with_mode(JamNodeMode::Validator)
            .build()
        {
            Ok(node) => Self::transition(
                JamchainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| ConfigError::Node(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Add a new ordinary node using a nested [`JamNodeConfigBuilder`].
    pub fn with_ordinary(
        self,
        f: impl FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
        match self
            .create_node_builder(f)
            .with_mode(JamNodeMode::Ordinary)
            .build()
        {
            Ok(node) => Self::transition(
                JamchainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| ConfigError::Node(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Add a new proxy node using a nested [`JamNodeConfigBuilder`].
    pub fn with_proxy(
        self,
        f: impl FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
        match self
            .create_node_builder(f)
            .with_mode(JamNodeMode::Proxy)
            .build()
        {
            Ok(node) => Self::transition(
                JamchainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| ConfigError::Node(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    // /// Helper fn to setup nodes (validators / ordinary / proxy )
    // pub fn with_nodes_setup<T>(
    //     self,
    //     val_count: usize,
    //     ordinary_count: usize,
    //     proxy_count: usize,
    // ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
    //     let mut nodes = vec![];
    //     let tot = val_count + ordinary_count + proxy_count;
    //     for i in 0..tot {}

    //     Self::transition(
    //         JamchainConfig {
    //             nodes,
    //             ..self.config
    //         },
    //         self.validation_context,
    //         self.errors,
    //     )
    // }
}

impl JamchainConfigBuilder<WithAtLeastOneNode> {
    /// Add a new validator node using a nested [`JamNodeConfigBuilder`].
    /// The node will be configured as a validator (authority).
    pub fn with_validator(
        self,
        f: impl FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
        match self
            .create_node_builder(f)
            .with_mode(JamNodeMode::Validator)
            .build()
        {
            Ok(node) => Self::transition(
                JamchainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| ConfigError::Node(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Add a new ordinary node using a nested [`JamNodeConfigBuilder`].
    pub fn with_ordinary(
        self,
        f: impl FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
        match self
            .create_node_builder(f)
            .with_mode(JamNodeMode::Ordinary)
            .build()
        {
            Ok(node) => Self::transition(
                JamchainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| ConfigError::Node(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Add a new proxy node using a nested [`JamNodeConfigBuilder`].
    pub fn with_proxy(
        self,
        f: impl FnOnce(JamNodeConfigBuilder<node::Initial>) -> JamNodeConfigBuilder<node::Buildable>,
    ) -> JamchainConfigBuilder<WithAtLeastOneNode> {
        match self
            .create_node_builder(f)
            .with_mode(JamNodeMode::Proxy)
            .build()
        {
            Ok(node) => Self::transition(
                JamchainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(
                    self.errors,
                    errors
                        .into_iter()
                        .map(|error| ConfigError::Node(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Seals the builder and returns a [`JamchainConfig`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<JamchainConfig, Vec<anyhow::Error>> {
        let errors = merge_errors_vecs(self.errors, self.config.spec_source_errors());
        if !errors.is_empty() {
            return Err(errors
                .into_iter()
                .map(|error| ConfigError::Jamchain(error).into())
                .collect::<Vec<_>>());
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> JamchainConfigBuilder<WithId> {
        JamchainConfigBuilder::new(Default::default()).with_id("dev")
    }

    #[test]
    fn the_genesis_is_absent_by_default() {
        let config = chain()
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        assert_eq!(config.genesis(), None);
    }

    /// Every call adds to the same genesis, so it can be split across the builder chain.
    #[test]
    fn the_genesis_is_built_up_across_calls() {
        let config = chain()
            .with_bootstrap_service("/blobs/bootstrap.jam")
            .with_genesis(|g| g.with_service(5, |s| s.with_code("/blobs/svc.jam")))
            .with_validator(|node| node.with_name("jam0"))
            .with_genesis(|g| g.with_assigner([0], 5))
            .build()
            .expect("the chain config builds");

        let genesis = config.genesis().expect("the genesis is set");
        assert_eq!(
            genesis.bootstrap_service(),
            Some(&AssetLocation::from("/blobs/bootstrap.jam"))
        );
        assert!(genesis.services().contains_key(&5));
        assert_eq!(genesis.privileges().assign()[&0], 5);
    }

    #[test]
    fn genesis_errors_are_reported() {
        let errors = chain()
            .with_genesis(|g| g.with_service(5, |s| s.with_min_item_gas(1)))
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].to_string(),
            "jamchain.genesis.services[5].code: a service needs its code"
        );
    }

    /// `genesis` customizes the built-in builder, so it can't be combined with a spec that is
    /// used as given.
    #[test]
    fn a_genesis_with_a_chain_spec_path_is_refused() {
        let errors = chain()
            .with_genesis(|g| g)
            .with_chain_spec_path("/spec.json")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .unwrap_err();

        assert!(
            errors[0]
                .to_string()
                .contains("genesis: can't be used with `chain_spec_path`"),
            "{}",
            errors[0]
        );
    }

    #[test]
    fn a_chain_spec_path_is_kept_as_given() {
        let config = chain()
            .with_chain_spec_path("/spec.json")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        assert_eq!(
            config.chain_spec_path(),
            Some(&AssetLocation::from("/spec.json"))
        );
        assert_eq!(config.genesis(), None);
    }
}
