use std::{cell::RefCell, error::Error, fmt::Debug, marker::PhantomData, rc::Rc};

use serde::{Deserialize, Serialize};
use support::constants::{DEFAULT_TYPESTATE, THIS_IS_A_BUG};

use crate::{
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{merge_errors, merge_errors_vecs},
        macros::states,
        node::{self, GroupNodeConfig, GroupNodeConfigBuilder, NodeConfig, NodeConfigBuilder},
        resources::{Resources, ResourcesBuilder},
        types::{
            Arg, AssetLocation, Chain, ChainDefaultContext, Command, Image, ValidationContext,
        },
    },
    utils::{default_command_polkadot, default_relaychain_chain, is_false},
};

/// A relay chain configuration, composed of nodes and fine-grained configuration options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelaychainConfig {
    #[serde(default = "default_relaychain_chain")]
    chain: Chain,
    #[serde(default = "default_command_polkadot")]
    default_command: Option<Command>,
    default_image: Option<Image>,
    default_resources: Option<Resources>,
    default_db_snapshot: Option<AssetLocation>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    default_args: Vec<Arg>,
    chain_spec_path: Option<AssetLocation>,
    // Full _template_ command, will be rendered (using custom token replacements)
    // and executed for generate the chain-spec.
    // available tokens {{chainName}} / {{disableBootnodes}}
    chain_spec_command: Option<String>,
    #[serde(skip_serializing_if = "is_false", default)]
    chain_spec_command_is_local: bool,
    random_nominators_count: Option<u32>,
    max_nominations: Option<u8>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    nodes: Vec<NodeConfig>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    node_groups: Vec<GroupNodeConfig>,
    #[serde(rename = "genesis", skip_serializing_if = "Option::is_none")]
    runtime_genesis_patch: Option<serde_json::Value>,
    // Path or url to override the runtime (:code) in the chain-spec
    wasm_override: Option<AssetLocation>,
    command: Option<Command>,
}

impl RelaychainConfig {
    /// The chain name.
    pub fn chain(&self) -> &Chain {
        &self.chain
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

    /// The default database snapshot location that will be used for state.
    pub fn default_db_snapshot(&self) -> Option<&AssetLocation> {
        self.default_db_snapshot.as_ref()
    }

    /// The default arguments that will be used to launch the node command.
    pub fn default_args(&self) -> Vec<&Arg> {
        self.default_args.iter().collect::<Vec<&Arg>>()
    }

    /// The location of an pre-existing chain specification for the relay chain.
    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    /// The location of a wasm runtime to override in the chain-spec.
    pub fn wasm_override(&self) -> Option<&AssetLocation> {
        self.wasm_override.as_ref()
    }

    /// The full _template_ command to genera the chain-spec
    pub fn chain_spec_command(&self) -> Option<&str> {
        self.chain_spec_command.as_deref()
    }

    /// Does the chain_spec_command needs to be run locally
    pub fn chain_spec_command_is_local(&self) -> bool {
        self.chain_spec_command_is_local
    }

    /// The non-default command used for nodes.
    pub fn command(&self) -> Option<&Command> {
        self.command.as_ref()
    }

    /// The number of `random nominators` to create for chains using staking, this is used in tandem with `max_nominations` to simulate the amount of nominators and nominations.
    pub fn random_nominators_count(&self) -> Option<u32> {
        self.random_nominators_count
    }

    /// The maximum number of nominations to create per nominator.
    pub fn max_nominations(&self) -> Option<u8> {
        self.max_nominations
    }

    /// The genesis overrides as a JSON value.
    pub fn runtime_genesis_patch(&self) -> Option<&serde_json::Value> {
        self.runtime_genesis_patch.as_ref()
    }

    /// The nodes of the relay chain.
    pub fn nodes(&self) -> Vec<&NodeConfig> {
        self.nodes.iter().collect::<Vec<&NodeConfig>>()
    }

    /// The group nodes of the relay chain.
    pub fn group_node_configs(&self) -> Vec<&GroupNodeConfig> {
        self.node_groups.iter().collect::<Vec<&GroupNodeConfig>>()
    }

    pub(crate) fn set_nodes(&mut self, nodes: Vec<NodeConfig>) {
        self.nodes = nodes;
    }
}

states! {
    Initial,
    WithChain,
    WithAtLeastOneNode
}

/// A relay chain configuration builder, used to build a [`RelaychainConfig`] declaratively with fields validation.
pub struct RelaychainConfigBuilder<State> {
    config: RelaychainConfig,
    validation_context: Rc<RefCell<ValidationContext>>,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<State>,
}

impl Default for RelaychainConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: RelaychainConfig {
                chain: "default"
                    .try_into()
                    .expect(&format!("{DEFAULT_TYPESTATE} {THIS_IS_A_BUG}")),
                default_command: None,
                default_image: None,
                default_resources: None,
                default_db_snapshot: None,
                default_args: vec![],
                chain_spec_path: None,
                chain_spec_command: None,
                wasm_override: None,
                chain_spec_command_is_local: false, // remote cmd by default
                command: None,
                random_nominators_count: None,
                max_nominations: None,
                runtime_genesis_patch: None,
                nodes: vec![],
                node_groups: vec![],
            },
            validation_context: Default::default(),
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> RelaychainConfigBuilder<A> {
    fn transition<B>(
        config: RelaychainConfig,
        validation_context: Rc<RefCell<ValidationContext>>,
        errors: Vec<anyhow::Error>,
    ) -> RelaychainConfigBuilder<B> {
        RelaychainConfigBuilder {
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
            default_db_snapshot: self.config.default_db_snapshot.clone(),
            default_args: self.config.default_args.clone(),
        }
    }
}

impl RelaychainConfigBuilder<Initial> {
    pub fn new(
        validation_context: Rc<RefCell<ValidationContext>>,
    ) -> RelaychainConfigBuilder<Initial> {
        Self {
            validation_context,
            ..Self::default()
        }
    }

    /// Set the chain name (e.g. rococo-local).
    pub fn with_chain<T>(self, chain: T) -> RelaychainConfigBuilder<WithChain>
    where
        T: TryInto<Chain>,
        T::Error: Error + Send + Sync + 'static,
    {
        match chain.try_into() {
            Ok(chain) => Self::transition(
                RelaychainConfig {
                    chain,
                    ..self.config
                },
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

impl RelaychainConfigBuilder<WithChain> {
    /// Set the default command used for nodes. Can be overridden.
    pub fn with_default_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                RelaychainConfig {
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
                RelaychainConfig {
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
                RelaychainConfig {
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

    /// Set the default database snapshot location that will be used for state. Can be overridden.
    pub fn with_default_db_snapshot(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            RelaychainConfig {
                default_db_snapshot: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the default arguments that will be used to execute the node command. Can be overridden.
    pub fn with_default_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            RelaychainConfig {
                default_args: args,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the location of a pre-existing chain specification for the relay chain.
    pub fn with_chain_spec_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            RelaychainConfig {
                chain_spec_path: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the location of a wasm to override the chain-spec.
    pub fn with_wasm_override(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            RelaychainConfig {
                wasm_override: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the chain-spec command _template_ for the relay chain.
    pub fn with_chain_spec_command(self, cmd_template: impl Into<String>) -> Self {
        Self::transition(
            RelaychainConfig {
                chain_spec_command: Some(cmd_template.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set if the chain-spec command needs to be run locally or not (false by default)
    pub fn chain_spec_command_is_local(self, choice: bool) -> Self {
        Self::transition(
            RelaychainConfig {
                chain_spec_command_is_local: choice,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the number of `random nominators` to create for chains using staking, this is used in tandem with `max_nominations` to simulate the amount of nominators and nominations.
    pub fn with_random_nominators_count(self, random_nominators_count: u32) -> Self {
        Self::transition(
            RelaychainConfig {
                random_nominators_count: Some(random_nominators_count),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the maximum number of nominations to create per nominator.
    pub fn with_max_nominations(self, max_nominations: u8) -> Self {
        Self::transition(
            RelaychainConfig {
                max_nominations: Some(max_nominations),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the genesis overrides as a JSON object.
    pub fn with_genesis_overrides(self, genesis_overrides: impl Into<serde_json::Value>) -> Self {
        Self::transition(
            RelaychainConfig {
                runtime_genesis_patch: Some(genesis_overrides.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Add a new node using a nested [`NodeConfigBuilder`].
    pub fn with_node(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> RelaychainConfigBuilder<WithAtLeastOneNode> {
        match f(NodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(node) => Self::transition(
                RelaychainConfig {
                    nodes: vec![node],
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

    /// Add a new group node using a nested [`GroupNodeConfigBuilder`].
    pub fn with_node_group(
        self,
        f: impl FnOnce(GroupNodeConfigBuilder<node::Initial>) -> GroupNodeConfigBuilder<node::Buildable>,
    ) -> RelaychainConfigBuilder<WithAtLeastOneNode> {
        match f(GroupNodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(group_node) => Self::transition(
                RelaychainConfig {
                    node_groups: vec![group_node],
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
}

impl RelaychainConfigBuilder<WithAtLeastOneNode> {
    /// Add a new node using a nested [`NodeConfigBuilder`].
    pub fn with_node(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match f(NodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(node) => Self::transition(
                RelaychainConfig {
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

    /// Add a new group node using a nested [`GroupNodeConfigBuilder`].
    pub fn with_node_group(
        self,
        f: impl FnOnce(GroupNodeConfigBuilder<node::Initial>) -> GroupNodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match f(GroupNodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(group_node) => Self::transition(
                RelaychainConfig {
                    node_groups: [self.config.node_groups, vec![group_node]].concat(),
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

    /// Seals the builder and returns a [`RelaychainConfig`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<RelaychainConfig, Vec<anyhow::Error>> {
        if !self.errors.is_empty() {
            return Err(self
                .errors
                .into_iter()
                .map(|error| ConfigError::Relaychain(error).into())
                .collect::<Vec<_>>());
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaychain_config_builder_should_succeeds_and_returns_a_relaychain_config() {
        let relaychain_config = RelaychainConfigBuilder::new(Default::default())
            .with_chain("polkadot")
            .with_default_image("myrepo:myimage")
            .with_default_command("default_command")
            .with_default_resources(|resources| {
                resources
                    .with_limit_cpu("500M")
                    .with_limit_memory("1G")
                    .with_request_cpu("250M")
            })
            .with_default_db_snapshot("https://www.urltomysnapshot.com/file.tgz")
            .with_chain_spec_path("./path/to/chain/spec.json")
            .with_wasm_override("./path/to/override/runtime.wasm")
            .with_default_args(vec![("--arg1", "value1").into(), "--option2".into()])
            .with_random_nominators_count(42)
            .with_max_nominations(5)
            .with_node(|node| node.with_name("node1").bootnode(true))
            .with_node(|node| {
                node.with_name("node2")
                    .with_command("command2")
                    .validator(true)
            })
            .build()
            .unwrap();

        assert_eq!(relaychain_config.chain().as_str(), "polkadot");
        assert_eq!(relaychain_config.nodes().len(), 2);
        let &node1 = relaychain_config.nodes().first().unwrap();
        assert_eq!(node1.name(), "node1");
        assert_eq!(node1.command().unwrap().as_str(), "default_command");
        assert!(node1.is_bootnode());
        let &node2 = relaychain_config.nodes().last().unwrap();
        assert_eq!(node2.name(), "node2");
        assert_eq!(node2.command().unwrap().as_str(), "command2");
        assert!(node2.is_validator());
        assert_eq!(
            relaychain_config.default_command().unwrap().as_str(),
            "default_command"
        );
        assert_eq!(
            relaychain_config.default_image().unwrap().as_str(),
            "myrepo:myimage"
        );
        let default_resources = relaychain_config.default_resources().unwrap();
        assert_eq!(default_resources.limit_cpu().unwrap().as_str(), "500M");
        assert_eq!(default_resources.limit_memory().unwrap().as_str(), "1G");
        assert_eq!(default_resources.request_cpu().unwrap().as_str(), "250M");
        assert!(matches!(
            relaychain_config.default_db_snapshot().unwrap(),
            AssetLocation::Url(value) if value.as_str() == "https://www.urltomysnapshot.com/file.tgz",
        ));
        assert!(matches!(
            relaychain_config.chain_spec_path().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/chain/spec.json"
        ));
        assert!(matches!(
            relaychain_config.wasm_override().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/override/runtime.wasm"
        ));
        let args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        assert_eq!(
            relaychain_config.default_args(),
            args.iter().collect::<Vec<_>>()
        );
        assert_eq!(relaychain_config.random_nominators_count().unwrap(), 42);
        assert_eq!(relaychain_config.max_nominations().unwrap(), 5);
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_chain_is_invalid() {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("invalid chain")
            .with_node(|node| {
                node.with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "relaychain.chain: 'invalid chain' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_default_command_is_invalid() {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_default_command("invalid command")
            .with_node(|node| {
                node.with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "relaychain.default_command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_default_image_is_invalid() {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_default_image("invalid image")
            .with_node(|node| {
                node.with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"relaychain.default_image: 'invalid image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_default_resources_are_invalid(
    ) {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_default_resources(|default_resources| {
                default_resources
                    .with_limit_memory("100m")
                    .with_request_cpu("invalid")
            })
            .with_node(|node| {
                node.with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            r"relaychain.default_resources.request_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_first_node_is_invalid() {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_node(|node| {
                node.with_name("node")
                    .with_command("invalid command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "relaychain.nodes['node'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_with_at_least_one_node_should_fails_and_returns_an_error_if_second_node_is_invalid(
    ) {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_node(|node| {
                node.with_name("node1")
                    .with_command("command1")
                    .validator(true)
            })
            .with_node(|node| {
                node.with_name("node2")
                    .with_command("invalid command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "relaychain.nodes['node2'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_returns_multiple_errors_if_a_node_and_default_resources_are_invalid(
    ) {
        let errors = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_default_resources(|resources| {
                resources
                    .with_request_cpu("100Mi")
                    .with_limit_memory("1Gi")
                    .with_limit_cpu("invalid")
            })
            .with_node(|node| {
                node.with_name("node")
                    .with_image("invalid image")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "relaychain.default_resources.limit_cpu: 'invalid' doesn't match regex '^\\d+(.\\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "relaychain.nodes['node'].image: 'invalid image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn relaychain_config_builder_should_works_with_chain_spec_command() {
        const CMD_TPL: &str = "./bin/chain-spec-generator {% raw %} {{chainName}} {% endraw %}";
        let config = RelaychainConfigBuilder::new(Default::default())
            .with_chain("polkadot")
            .with_default_image("myrepo:myimage")
            .with_default_command("default_command")
            .with_chain_spec_command(CMD_TPL)
            .with_node(|node| node.with_name("node1").bootnode(true))
            .build()
            .unwrap();

        assert_eq!(config.chain_spec_command(), Some(CMD_TPL));
        assert!(!config.chain_spec_command_is_local());
    }

    #[test]
    fn relaychain_config_builder_should_works_with_chain_spec_command_locally() {
        const CMD_TPL: &str = "./bin/chain-spec-generator {% raw %} {{chainName}} {% endraw %}";
        let config = RelaychainConfigBuilder::new(Default::default())
            .with_chain("polkadot")
            .with_default_image("myrepo:myimage")
            .with_default_command("default_command")
            .with_chain_spec_command(CMD_TPL)
            .chain_spec_command_is_local(true)
            .with_node(|node| node.with_name("node1").bootnode(true))
            .build()
            .unwrap();

        assert_eq!(config.chain_spec_command(), Some(CMD_TPL));
        assert!(config.chain_spec_command_is_local());
    }

    #[test]
    fn relaychain_with_group_config_should_succeeds_and_returns_a_relaychain_config() {
        let relaychain_config = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_default_command("command")
            .with_node(|node| {
                node.with_name("node")
                    .with_command("node_command")
                    .validator(true)
            })
            .with_node_group(|group| {
                group.with_count(2).with_base_node(|base| {
                    base.with_name("group_node")
                        .with_command("some_command")
                        .with_image("repo:image")
                        .validator(true)
                })
            })
            .build()
            .unwrap();

        assert_eq!(relaychain_config.chain().as_str(), "chain");
        assert_eq!(relaychain_config.nodes().len(), 1);
        assert_eq!(relaychain_config.group_node_configs().len(), 1);
        assert_eq!(
            relaychain_config
                .group_node_configs()
                .first()
                .unwrap()
                .count,
            2
        );
        let &node = relaychain_config.nodes().first().unwrap();
        assert_eq!(node.name(), "node");
        assert_eq!(node.command().unwrap().as_str(), "node_command");

        let group_nodes = relaychain_config.group_node_configs();
        let group_base_node = group_nodes.first().unwrap();
        assert_eq!(group_base_node.base_config.name(), "group_node");
        assert_eq!(
            group_base_node.base_config.command().unwrap().as_str(),
            "some_command"
        );
        assert_eq!(
            group_base_node.base_config.image().unwrap().as_str(),
            "repo:image"
        );
        assert!(group_base_node.base_config.is_validator());
    }

    #[test]
    fn relaychain_with_group_count_0_config_should_fail() {
        let relaychain_config = RelaychainConfigBuilder::new(Default::default())
            .with_chain("chain")
            .with_default_command("command")
            .with_node(|node| {
                node.with_name("node")
                    .with_command("node_command")
                    .validator(true)
            })
            .with_node_group(|group| {
                group.with_count(0).with_base_node(|base| {
                    base.with_name("group_node")
                        .with_command("some_command")
                        .with_image("repo:image")
                        .validator(true)
                })
            })
            .build();

        let errors: Vec<anyhow::Error> = match relaychain_config {
            Ok(_) => vec![],
            Err(errs) => errs,
        };

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "relaychain.nodes['group_node'].Count cannot be zero"
        );
    }
}
