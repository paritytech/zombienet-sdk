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
    /// chain-spec to use (location can be url or file path).
    chain_spec_path: Option<AssetLocation>,
    /// Generate the spec with an external `<cmd> gen-spec` instead of the built-in builder.
    chain_spec_command: Option<Command>,
    /// Command to spawn corevm monitor (if present)
    corevm_monitor_command: Option<Command>,
    /// Command to spawn corevm builder (if present)
    corevm_builder_command: Option<Command>,
    /// Genesis state beyond the validator set, merged into the config `gen-spec` reads.
    /// Only used by an external generator (polkajam's own `gen-spec` ignores it); setting it
    /// opts into generating the spec with `gen-spec`.
    #[serde(skip_serializing_if = "Option::is_none")]
    genesis_overrides: Option<serde_json::Value>,
    /// Customizations of the genesis built in process (the default way to build the spec).
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

    /// The location of an pre-existing chain specification for the relay chain.
    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    /// The command generating the spec with `gen-spec`, instead of the built-in builder.
    pub fn chain_spec_command(&self) -> Option<&Command> {
        self.chain_spec_command.as_ref()
    }

    /// The corevm monitor command used (if set).
    pub fn corevm_monitor_command(&self) -> Option<&Command> {
        self.corevm_monitor_command.as_ref()
    }

    /// The corevm builder command used (if set).
    pub fn corevm_builder_command(&self) -> Option<&Command> {
        self.corevm_builder_command.as_ref()
    }

    /// The genesis overrides merged into the config `gen-spec` reads (if set).
    pub fn genesis_overrides(&self) -> Option<&serde_json::Value> {
        self.genesis_overrides.as_ref()
    }

    /// The customizations of the genesis built in process (if set).
    pub fn genesis(&self) -> Option<&JamGenesisConfig> {
        self.genesis.as_ref()
    }

    /// The nodes of the relay chain.
    pub fn nodes(&self) -> Vec<&JamNodeConfig> {
        self.nodes.iter().collect::<Vec<&JamNodeConfig>>()
    }

    /// Check that the fields choosing how the spec is produced don't conflict: a pre-existing
    /// spec (`chain_spec_path`), an external generator (`chain_spec_command` /
    /// `genesis_overrides`) or the built-in builder (the default, customized by `genesis`).
    pub(crate) fn spec_source_errors(&self) -> Vec<anyhow::Error> {
        let mut errors = vec![];
        let generator_field = if self.chain_spec_command.is_some() {
            Some("chain_spec_command")
        } else if self.genesis_overrides.is_some() {
            Some("genesis_overrides")
        } else {
            None
        };

        if self.genesis.is_some() {
            if self.chain_spec_path.is_some() {
                errors.push(anyhow::anyhow!(
                    "genesis: can't be used with `chain_spec_path`, the spec is used as given"
                ));
            }
            if let Some(field) = generator_field {
                errors.push(anyhow::anyhow!(
                    "genesis: can't be used with `{field}`, which generates the spec with \
                     `gen-spec` instead of the built-in builder"
                ));
            }
        } else if let (Some(_), Some(field)) = (&self.chain_spec_path, generator_field) {
            errors.push(anyhow::anyhow!(
                "chain_spec_path: can't be used with `{field}`, the spec is used as given"
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

    /// Set the location of a pre-existing chain specification for the chain.
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

    /// Generate the chain spec with this command's `gen-spec`, instead of building it in
    /// process (the default).
    ///
    /// Only the command is used: the orchestrator appends `gen-spec <config> <spec>` itself.
    pub fn with_chain_spec_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                JamchainConfig {
                    chain_spec_command: Some(command),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(self.errors, FieldError::Command(error.into()).into()),
            ),
        }
    }

    /// Set the genesis overrides: a JSON object merged into the config `gen-spec` reads, on top
    /// of the generated `id` and `genesis_validators`. Its keys are `gen-spec`'s business.
    ///
    /// Opts into generating the spec with `gen-spec`; to customize the genesis built in process
    /// use [`Self::with_genesis`].
    pub fn with_genesis_overrides(self, genesis_overrides: impl Into<serde_json::Value>) -> Self {
        Self::transition(
            JamchainConfig {
                genesis_overrides: Some(genesis_overrides.into()),
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
    /// Set the genesis overrides: a JSON object merged into the config `gen-spec` reads, on top
    /// of the generated `id` and `genesis_validators`. Its keys are `gen-spec`'s business.
    ///
    /// Opts into generating the spec with `gen-spec`; to customize the genesis built in process
    /// use [`Self::with_genesis`].
    pub fn with_genesis_overrides(self, genesis_overrides: impl Into<serde_json::Value>) -> Self {
        Self::transition(
            JamchainConfig {
                genesis_overrides: Some(genesis_overrides.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
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
    use serde_json::json;

    use super::*;

    fn overrides() -> serde_json::Value {
        json!({
            "services": [{ "id": 5, "code": "/blobs/parasim-service.jam" }],
            "assigners": { "0": 5 },
        })
    }

    /// The override is the caller's business: it reaches `gen-spec` untouched, so nothing here
    /// knows or checks what its keys mean.
    #[test]
    fn genesis_overrides_are_kept_as_given() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_genesis_overrides(overrides())
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        assert_eq!(config.genesis_overrides(), Some(&overrides()));
    }

    /// Callers set the override after the nodes as naturally as before them.
    #[test]
    fn genesis_overrides_can_follow_the_nodes() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_validator(|node| node.with_name("jam0"))
            .with_genesis_overrides(overrides())
            .build()
            .expect("the chain config builds");

        assert_eq!(config.genesis_overrides(), Some(&overrides()));
    }

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

    /// `genesis` customizes the built-in builder, so it can't be combined with the fields that
    /// pick another way to produce the spec.
    #[test]
    fn conflicting_spec_sources_are_refused() {
        let cases: Vec<(JamchainConfigBuilder<WithId>, &str)> = vec![
            (
                chain()
                    .with_genesis(|g| g)
                    .with_chain_spec_path("/spec.json"),
                "genesis: can't be used with `chain_spec_path`",
            ),
            (
                chain()
                    .with_genesis(|g| g)
                    .with_chain_spec_command("polkajam"),
                "genesis: can't be used with `chain_spec_command`",
            ),
            (
                chain()
                    .with_chain_spec_path("/spec.json")
                    .with_chain_spec_command("polkajam"),
                "chain_spec_path: can't be used with `chain_spec_command`",
            ),
        ];
        for (builder, expected) in cases {
            let errors = builder
                .with_validator(|node| node.with_name("jam0"))
                .build()
                .unwrap_err();
            assert!(errors[0].to_string().contains(expected), "{}", errors[0]);
        }

        let errors = chain()
            .with_genesis(|g| g)
            .with_validator(|node| node.with_name("jam0"))
            .with_genesis_overrides(overrides())
            .build()
            .unwrap_err();
        assert!(errors[0]
            .to_string()
            .contains("genesis: can't be used with `genesis_overrides`"));
    }

    #[test]
    fn genesis_overrides_are_absent_by_default() {
        let config = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_validator(|node| node.with_name("jam0"))
            .build()
            .expect("the chain config builds");

        assert_eq!(config.genesis_overrides(), None);
    }
}
