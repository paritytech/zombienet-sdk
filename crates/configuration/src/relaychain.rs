use std::{error::Error, fmt::Debug, marker::PhantomData};

use crate::shared::{
    errors::{ConfigError, FieldError},
    helpers::{merge_errors, merge_errors_vecs},
    macros::states,
    node::{self, NodeConfig, NodeConfigBuilder},
    resources::{Resources, ResourcesBuilder},
    types::{Arg, AssetLocation, Chain, ChainDefaultContext, Command, Image},
};

/// A relaychain configuration, composed of nodes and fine-grained configuration options.
#[derive(Debug, Clone, PartialEq)]
pub struct RelaychainConfig {
    /// Chain to use (e.g. rococo-local).
    chain: Chain,

    /// Default command to run the node. Can be overriden on each node.
    default_command: Option<Command>,

    /// Default image to use (only podman/k8s). Can be overriden on each node.
    default_image: Option<Image>,

    /// Default resources. Can be overriden on each node.
    default_resources: Option<Resources>,

    /// Default database snapshot. Can be overriden on each node.
    default_db_snapshot: Option<AssetLocation>,

    /// Default arguments to use in nodes. Can be overriden on each node.
    default_args: Vec<Arg>,

    /// Chain specification JSON file to use.
    chain_spec_path: Option<AssetLocation>,

    /// Set the count of nominators to generator (used with PoS networks).
    random_nominators_count: Option<u32>,

    /// Set the max nominators value (used with PoS networks).
    max_nominations: Option<u8>,

    /// Nodes to run.
    nodes: Vec<NodeConfig>,
}

impl RelaychainConfig {
    pub fn chain(&self) -> &Chain {
        &self.chain
    }

    pub fn default_command(&self) -> Option<&Command> {
        self.default_command.as_ref()
    }

    pub fn default_image(&self) -> Option<&Image> {
        self.default_image.as_ref()
    }

    pub fn default_resources(&self) -> Option<&Resources> {
        self.default_resources.as_ref()
    }

    pub fn default_db_snapshot(&self) -> Option<&AssetLocation> {
        self.default_db_snapshot.as_ref()
    }

    pub fn default_args(&self) -> Vec<&Arg> {
        self.default_args.iter().collect::<Vec<&Arg>>()
    }

    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    pub fn random_minators_count(&self) -> Option<u32> {
        self.random_nominators_count
    }

    pub fn max_nominations(&self) -> Option<u8> {
        self.max_nominations
    }

    pub fn nodes(&self) -> Vec<&NodeConfig> {
        self.nodes.iter().collect::<Vec<&NodeConfig>>()
    }
}

states! {
    Initial,
    WithChain,
    WithAtLeastOneNode
}

#[derive(Debug)]
pub struct RelaychainConfigBuilder<State> {
    config: RelaychainConfig,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<State>,
}

impl Default for RelaychainConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: RelaychainConfig {
                chain: "default"
                    .try_into()
                    .expect("'default' overriding should be ensured by typestate. this is a bug, please report it: https://github.com/paritytech/zombienet-sdk/issues"),
                default_command: None,
                default_image: None,
                default_resources: None,
                default_db_snapshot: None,
                default_args: vec![],
                chain_spec_path: None,
                random_nominators_count: None,
                max_nominations: None,
                nodes: vec![],
            },
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> RelaychainConfigBuilder<A> {
    fn transition<B>(
        config: RelaychainConfig,
        errors: Vec<anyhow::Error>,
    ) -> RelaychainConfigBuilder<B> {
        RelaychainConfigBuilder {
            config,
            errors,
            _state: PhantomData,
        }
    }

    fn default_context(&self) -> ChainDefaultContext {
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
    pub fn new() -> RelaychainConfigBuilder<Initial> {
        Self::default()
    }

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
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::Chain(error.into()).into()),
            ),
        }
    }
}

impl RelaychainConfigBuilder<WithChain> {
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
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::DefaultCommand(error.into()).into()),
            ),
        }
    }

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
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::DefaultImage(error.into()).into()),
            ),
        }
    }

    pub fn with_default_resources(self, f: fn(ResourcesBuilder) -> ResourcesBuilder) -> Self {
        match f(ResourcesBuilder::new()).build() {
            Ok(default_resources) => Self::transition(
                RelaychainConfig {
                    default_resources: Some(default_resources),
                    ..self.config
                },
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
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

    pub fn with_default_db_snapshot(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            RelaychainConfig {
                default_db_snapshot: Some(location.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_default_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            RelaychainConfig {
                default_args: args,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_chain_spec_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            RelaychainConfig {
                chain_spec_path: Some(location.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_random_nominators_count(self, random_nominators_count: u32) -> Self {
        Self::transition(
            RelaychainConfig {
                random_nominators_count: Some(random_nominators_count),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_max_nominations(self, max_nominations: u8) -> Self {
        Self::transition(
            RelaychainConfig {
                max_nominations: Some(max_nominations),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_node(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> RelaychainConfigBuilder<WithAtLeastOneNode> {
        match f(NodeConfigBuilder::new(self.default_context())).build() {
            Ok(node) => Self::transition(
                RelaychainConfig {
                    nodes: vec![node],
                    ..self.config
                },
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
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
    pub fn with_node(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match f(NodeConfigBuilder::new(self.default_context())).build() {
            Ok(node) => Self::transition(
                RelaychainConfig {
                    nodes: [self.config.nodes, vec![node]].concat(),
                    ..self.config
                },
                self.errors,
            ),
            Err((name, errors)) => Self::transition(
                self.config,
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
        let relaychain_config = RelaychainConfigBuilder::new()
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
        let args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        assert_eq!(
            relaychain_config.default_args(),
            args.iter().collect::<Vec<_>>()
        );
        assert_eq!(relaychain_config.random_minators_count().unwrap(), 42);
        assert_eq!(relaychain_config.max_nominations().unwrap(), 5);
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_chain_is_invalid() {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            "relaychain.chain: 'invalid chain' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_default_command_is_invalid() {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            "relaychain.default_command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_default_image_is_invalid() {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            r"relaychain.default_image: 'invalid image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_default_resources_are_invalid(
    ) {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            r"relaychain.default_resources.request_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_and_returns_an_error_if_first_node_is_invalid() {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            "relaychain.nodes['node'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_with_at_least_one_node_should_fails_and_returns_an_error_if_second_node_is_invalid(
    ) {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            "relaychain.nodes['node2'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn relaychain_config_builder_should_fails_returns_multiple_errors_if_a_node_and_default_resources_are_invalid(
    ) {
        let errors = RelaychainConfigBuilder::new()
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
            errors.get(0).unwrap().to_string(),
            "relaychain.default_resources.limit_cpu: 'invalid' doesn't match regex '^\\d+(.\\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "relaychain.nodes['node'].image: 'invalid image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }
}
