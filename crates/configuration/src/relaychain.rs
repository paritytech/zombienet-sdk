use std::{fmt::Debug, marker::PhantomData};

use crate::shared::{
    macros::states,
    node::{self, NodeConfig, NodeConfigBuilder},
    resources::{Resources, ResourcesBuilder},
    types::{Arg, AssetLocation},
};

/// A relaychain configuration, composed of nodes and fine-grained configuration options.
#[derive(Debug, Clone, PartialEq)]
pub struct RelaychainConfig {
    /// Chain to use (e.g. rococo-local).
    chain: String,

    /// Default command to run the node. Can be overriden on each node.
    default_command: Option<String>,

    /// Default image to use (only podman/k8s). Can be overriden on each node.
    default_image: Option<String>,

    /// Default resources. Can be overriden on each node.
    default_resources: Option<Resources>,

    /// Default database snapshot. Can be overriden on each node.
    default_db_snapshot: Option<AssetLocation>,

    /// Chain specification JSON file to use.
    chain_spec_path: Option<AssetLocation>,

    /// Default arguments to use in nodes. Can be overriden on each node.
    default_args: Vec<Arg>,

    /// Set the count of nominators to generator (used with PoS networks).
    random_nominators_count: Option<u32>,

    /// Set the max nominators value (used with PoS networks).
    max_nominations: Option<u8>,

    /// Nodes to run.
    nodes: Vec<NodeConfig>,
}

impl RelaychainConfig {
    pub fn chain(&self) -> &str {
        &self.chain
    }

    pub fn default_command(&self) -> Option<&str> {
        self.default_command.as_deref()
    }

    pub fn default_image(&self) -> Option<&str> {
        self.default_image.as_deref()
    }

    pub fn default_resources(&self) -> Option<&Resources> {
        self.default_resources.as_ref()
    }

    pub fn default_db_snapshot(&self) -> Option<&AssetLocation> {
        self.default_db_snapshot.as_ref()
    }

    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    pub fn default_args(&self) -> Vec<&Arg> {
        self.default_args.iter().collect::<Vec<&Arg>>()
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
    WithAtLeastOneNode,
    WithDefaultCommand,
    WithDefaultCommandAndAtLeastOneNode
}

#[derive(Debug)]
pub struct RelaychainConfigBuilder<State> {
    config: RelaychainConfig,
    _state: PhantomData<State>,
}

impl Default for RelaychainConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: RelaychainConfig {
                chain:                   "".into(),
                default_command:         None,
                default_image:           None,
                default_resources:       None,
                default_db_snapshot:     None,
                chain_spec_path:         None,
                default_args:            vec![],
                random_nominators_count: None,
                max_nominations:         None,
                nodes:                   vec![],
            },
            _state: PhantomData,
        }
    }
}

impl<A> RelaychainConfigBuilder<A> {
    fn transition<B>(config: RelaychainConfig) -> RelaychainConfigBuilder<B> {
        RelaychainConfigBuilder {
            config,
            _state: PhantomData,
        }
    }
}

impl RelaychainConfigBuilder<Initial> {
    pub fn new() -> RelaychainConfigBuilder<Initial> {
        Self::default()
    }

    pub fn with_chain(self, chain: &str) -> RelaychainConfigBuilder<WithChain> {
        Self::transition(RelaychainConfig {
            chain: chain.to_owned(),
            ..self.config
        })
    }
}

impl RelaychainConfigBuilder<WithChain> {
    pub fn with_default_command(
        self,
        command: &str,
    ) -> RelaychainConfigBuilder<WithDefaultCommand> {
        Self::transition(RelaychainConfig {
            default_command: Some(command.to_owned()),
            ..self.config
        })
    }

    pub fn with_node(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> RelaychainConfigBuilder<WithAtLeastOneNode> {
        let new_node = f(NodeConfigBuilder::new()).build();
        Self::transition(RelaychainConfig {
            nodes: vec![new_node],
            ..self.config
        })
    }
}

impl RelaychainConfigBuilder<WithDefaultCommand> {
    pub fn with_node(
        self,
        f: fn(NodeConfigBuilder<node::WithDefaultCommand>) -> NodeConfigBuilder<node::Buildable>,
    ) -> RelaychainConfigBuilder<WithDefaultCommandAndAtLeastOneNode> {
        let new_node = f(NodeConfigBuilder::new_with_default_command()).build();
        Self::transition(RelaychainConfig {
            nodes: vec![new_node],
            ..self.config
        })
    }
}

macro_rules! non_changing_state_methods {
    () => {
        pub fn with_default_image(self, image: impl Into<String>) -> Self {
            Self::transition(RelaychainConfig {
                default_image: Some(image.into()),
                ..self.config
            })
        }

        pub fn with_default_resources(self, f: fn(ResourcesBuilder) -> ResourcesBuilder) -> Self {
            let default_resources = Some(f(ResourcesBuilder::new()).build());

            Self::transition(RelaychainConfig {
                default_resources,
                ..self.config
            })
        }

        pub fn with_default_db_snapshot(self, location: AssetLocation) -> Self {
            Self::transition(RelaychainConfig {
                default_db_snapshot: Some(location),
                ..self.config
            })
        }

        pub fn with_chain_spec_path(self, chain_spec_path: AssetLocation) -> Self {
            Self::transition(RelaychainConfig {
                chain_spec_path: Some(chain_spec_path),
                ..self.config
            })
        }

        pub fn with_default_args(self, args: Vec<Arg>) -> Self {
            Self::transition(RelaychainConfig {
                default_args: args,
                ..self.config
            })
        }

        pub fn with_random_nominators_count(self, random_nominators_count: u32) -> Self {
            Self::transition(RelaychainConfig {
                random_nominators_count: Some(random_nominators_count),
                ..self.config
            })
        }

        pub fn with_max_nominations(self, max_nominations: u8) -> Self {
            Self::transition(RelaychainConfig {
                max_nominations: Some(max_nominations),
                ..self.config
            })
        }
    };
}

impl RelaychainConfigBuilder<WithChain> {
    non_changing_state_methods!();
}

impl RelaychainConfigBuilder<WithDefaultCommand> {
    non_changing_state_methods!();
}

impl RelaychainConfigBuilder<WithAtLeastOneNode> {
    pub fn with_node(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        let new_node = f(NodeConfigBuilder::new()).build();

        Self::transition(RelaychainConfig {
            nodes: vec![self.config.nodes, vec![new_node]].concat(),
            ..self.config
        })
    }

    pub fn build(self) -> RelaychainConfig {
        self.config
    }
}

impl RelaychainConfigBuilder<WithDefaultCommandAndAtLeastOneNode> {
    pub fn with_node(
        self,
        f: fn(NodeConfigBuilder<node::WithDefaultCommand>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        let new_node = f(NodeConfigBuilder::new_with_default_command()).build();

        Self::transition(RelaychainConfig {
            nodes: vec![self.config.nodes, vec![new_node]].concat(),
            ..self.config
        })
    }

    pub fn build(self) -> RelaychainConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaychain_config_builder_should_build_a_new_relaychain_config_correctly() {
        let relaychain_config = RelaychainConfigBuilder::new()
            .with_chain("polkadot")
            .with_default_command("default_command")
            .with_default_image("myrepo:myimage")
            .with_default_resources(|resources| {
                resources
                    .with_limit_cpu("500M")
                    .with_limit_memory("1G")
                    .with_request_cpu("250M")
            })
            .with_default_db_snapshot(AssetLocation::Url(
                "https://www.urltomysnapshot.com/file.tgz".into(),
            ))
            .with_chain_spec_path(AssetLocation::FilePath("./path/to/chain/spec.json".into()))
            .with_default_args(vec![("--arg1", "value1").into(), "--option2".into()])
            .with_random_nominators_count(42)
            .with_max_nominations(5)
            .with_node(|node1| node1.with_name("node1").bootnode(true))
            .with_node(|node2| {
                node2
                    .with_name("node2")
                    .with_command("command2")
                    .validator(true)
            })
            .build();

        assert_eq!(relaychain_config.chain(), "polkadot");
        assert_eq!(relaychain_config.nodes().len(), 2);
        let &node1 = relaychain_config.nodes().first().unwrap();
        assert_eq!(node1.name(), "node1");
        // assert_eq!(node1.command().unwrap(), "command1");
        assert!(node1.is_bootnode());
        let &node2 = relaychain_config.nodes().last().unwrap();
        assert_eq!(node2.name(), "node2");
        assert_eq!(node2.command().unwrap(), "command2");
        assert!(node2.is_validator(), "node2");
        assert_eq!(
            relaychain_config.default_command().unwrap(),
            "default_command"
        );
        assert_eq!(relaychain_config.default_image().unwrap(), "myrepo:myimage");
        let default_resources = relaychain_config.default_resources().unwrap();
        assert_eq!(default_resources.limit_cpu().unwrap().value(), "500M");
        assert_eq!(default_resources.limit_memory().unwrap().value(), "1G");
        assert_eq!(default_resources.request_cpu().unwrap().value(), "250M");
        assert!(matches!(
            relaychain_config.default_db_snapshot().unwrap(),
            AssetLocation::Url(value) if value == "https://www.urltomysnapshot.com/file.tgz",
        ));
        assert!(matches!(
            relaychain_config.chain_spec_path().unwrap(),
            AssetLocation::FilePath(value) if value == "./path/to/chain/spec.json"
        ));
        let args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        assert_eq!(
            relaychain_config.default_args(),
            args.iter().collect::<Vec<_>>()
        );
        assert_eq!(relaychain_config.random_minators_count().unwrap(), 42);
        assert_eq!(relaychain_config.max_nominations().unwrap(), 5);
    }
}
