use std::{cell::RefCell, error::Error, fmt::Debug, marker::PhantomData, rc::Rc};

use serde::{Deserialize, Serialize};
use support::constants::{DEFAULT_TYPESTATE, THIS_IS_A_BUG};

use crate::{
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{merge_errors, merge_errors_vecs},
        macros::states,
        node::{
            self, JamNodeConfig, JamNodeConfigBuilder,
        },
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
    chain_spec_command: Option<Command>,
    /// Command to spawn corevm monitor (if present)
    corevm_monitor_command: Option<Command>,
    /// Command to spawn corevm builder (if present)
    corevm_builder_command: Option<Command>,
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

    /// The location of an pre-existing chain specification for the relay chain.
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

    /// The nodes of the relay chain.
    pub fn nodes(&self) -> Vec<&JamNodeConfig> {
        self.nodes.iter().collect::<Vec<&JamNodeConfig>>()
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
        if !self.errors.is_empty() {
            return Err(self
                .errors
                .into_iter()
                .map(|error| ConfigError::Jamchain(error).into())
                .collect::<Vec<_>>());
        }

        Ok(self.config)
    }
}
