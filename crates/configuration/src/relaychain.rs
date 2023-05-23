use crate::shared::{
    node::NodeConfig,
    types::{Arg, AssetLocation, Resources},
};

/// A relaychain configuration, composed of nodes and fine-grained configuration options.
#[derive(Debug, Default, Clone)]
pub struct RelaychainConfig {
    /// Default command to run the node. Can be overriden on each node.
    default_command: Option<String>,

    /// Default image to use (only podman/k8s). Can be overriden on each node.
    default_image: Option<String>,

    /// Default resources. Can be overriden on each node.
    default_resources: Option<Resources>,

    /// Default database snapshot. Can be overriden on each node.
    default_db_snapshot: Option<AssetLocation>,

    /// Chain to use (e.g. rococo-local).
    chain: String,

    /// Chain specification JSON file to use.
    chain_spec_path: Option<AssetLocation>,

    /// Default arguments to use in nodes. Can be overriden on each node.
    default_args: Vec<Arg>,

    /// Set the count of nominators to generator (used with PoS networks).
    random_nominators_count: Option<u32>,

    /// Set the max nominators value (used with PoS networks).
    max_nominations: Option<u16>,

    /// Nodes to run.
    nodes: Vec<NodeConfig>,
    // [TODO]: do we need node_groups in the sdk?
    // node_groups?: NodeGroupConfig[];

    // [TODO]: allow customize genesis
    // genesis?: JSON | ObjectJSON;
}

impl RelaychainConfig {
    pub fn with_default_command(self, command: &str) -> Self {
        Self {
            default_command: Some(command.to_owned()),
            ..self
        }
    }

    pub fn with_default_image(self, image: &str) -> Self {
        Self {
            default_image: Some(image.to_owned()),
            ..self
        }
    }

    pub fn with_default_resources(self, f: fn(Resources) -> Resources) -> Self {
        Self {
            default_resources: Some(f(Resources::default())),
            ..self
        }
    }

    pub fn with_default_db_snapshot(self, location: AssetLocation) -> Self {
        Self {
            default_db_snapshot: Some(location),
            ..self
        }
    }

    pub fn with_chain(self, chain: &str) -> Self {
        Self {
            chain: chain.to_owned(),
            ..self
        }
    }

    pub fn with_chain_spec_path(self, chain_spec_path: AssetLocation) -> Self {
        Self {
            chain_spec_path: Some(chain_spec_path),
            ..self
        }
    }

    pub fn with_default_args(self, args: Vec<Arg>) -> Self {
        Self {
            default_args: args,
            ..self
        }
    }

    pub fn with_random_nominators_count(self, random_nominators_count: u32) -> Self {
        Self {
            random_nominators_count: Some(random_nominators_count),
            ..self
        }
    }

    pub fn with_max_nominations(self, max_nominations: u16) -> Self {
        Self {
            max_nominations: Some(max_nominations),
            ..self
        }
    }

    pub fn with_node(self, f: fn(NodeConfig) -> NodeConfig) -> Self {
        Self {
            nodes: vec![self.nodes, vec![f(NodeConfig::default())]].concat(),
            ..self
        }
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

    pub fn chain(&self) -> &str {
        &self.chain
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

    pub fn max_nominations(&self) -> Option<u16> {
        self.max_nominations
    }

    pub fn nodes(&self) -> Vec<&NodeConfig> {
        self.nodes.iter().collect::<Vec<&NodeConfig>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_default_command_should_update_the_default_command_on_the_relaychain_config() {
        let relaychain_config =
            RelaychainConfig::default().with_default_command("my default command to run");

        assert_eq!(
            relaychain_config.default_command().unwrap(),
            "my default command to run"
        );
    }

    #[test]
    fn with_default_image_should_update_the_default_image_on_the_relaychain_config() {
        let relaychain_config =
            RelaychainConfig::default().with_default_image("myrepo:mydefaultimage");

        assert_eq!(
            relaychain_config.default_image().unwrap(),
            "myrepo:mydefaultimage"
        );
    }

    #[test]
    fn with_default_resources_should_update_the_default_resources_on_the_relaychain_config() {
        let relaychain_config =
            RelaychainConfig::default().with_default_resources(|default_resources| {
                default_resources
                    .with_limit_cpu("500M")
                    .with_limit_memory("1G")
            });

        assert_eq!(
            relaychain_config
                .default_resources()
                .unwrap()
                .limit_cpu()
                .unwrap()
                .value(),
            "500M"
        );
        assert_eq!(
            relaychain_config
                .default_resources()
                .unwrap()
                .limit_memory()
                .unwrap()
                .value(),
            "1G"
        );
        assert!(relaychain_config
            .default_resources()
            .unwrap()
            .request_cpu()
            .is_none());
        assert!(relaychain_config
            .default_resources()
            .unwrap()
            .request_memory()
            .is_none());
    }

    #[test]
    fn with_default_db_snapshot_should_update_the_default_db_snapshot_on_the_relaychain_config() {
        let location = AssetLocation::Url("https://www.mybackupwebsite.com/backup.tgz".into());
        let relaychain_config =
            RelaychainConfig::default().with_default_db_snapshot(location.clone());

        assert_eq!(relaychain_config.default_db_snapshot().unwrap(), &location);
    }

    #[test]
    fn with_chain_should_update_the_chain_on_the_relaychain_config() {
        let relaychain_config = RelaychainConfig::default().with_chain("mychainname");

        assert_eq!(relaychain_config.chain(), "mychainname");
    }

    #[test]
    fn with_chain_spec_path_should_update_the_chain_spec_path_on_the_relaychain_config() {
        let location = AssetLocation::FilePath("./folder1/folder2/mysuperchainspec.json".into());
        let relaychain_config = RelaychainConfig::default().with_chain_spec_path(location.clone());

        assert_eq!(relaychain_config.chain_spec_path().unwrap(), &location);
    }

    #[test]
    fn with_default_args_should_update_the_default_args_on_the_relaychain_config() {
        let default_args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        let relaychain_config = RelaychainConfig::default().with_default_args(default_args.clone());

        assert_eq!(
            relaychain_config.default_args(),
            default_args.iter().collect::<Vec<&Arg>>()
        );
    }

    #[test]
    fn with_random_nominators_count_should_update_the_random_nominators_count_on_the_relaychain_config(
    ) {
        let relaychain_config = RelaychainConfig::default().with_random_nominators_count(42);

        assert_eq!(relaychain_config.random_minators_count().unwrap(), 42);
    }

    #[test]
    fn with_max_nominations_should_update_the_max_nominations_on_the_relaychain_config() {
        let relaychain_config = RelaychainConfig::default().with_max_nominations(5);

        assert_eq!(relaychain_config.max_nominations().unwrap(), 5);
    }

    #[test]
    fn with_node_should_update_the_nodes_on_the_relaychain_config() {
        let relaychain_config = RelaychainConfig::default()
            .with_node(|node1| {
                node1
                    .being_bootnode()
                    .being_validator()
                    .with_name("mynode1")
                    .with_command("my command")
            })
            .with_node(|node2| {
                node2
                    .being_validator()
                    .with_name("mynode2")
                    .with_image("myrepo:mysuperimage")
            });

        let nodes = relaychain_config.nodes();

        assert_eq!(nodes.len(), 2);
        assert_eq!(
            *nodes.get(0).unwrap(),
            &NodeConfig::default()
                .being_bootnode()
                .being_validator()
                .with_name("mynode1")
                .with_command("my command")
        );
        assert_eq!(
            *nodes.get(1).unwrap(),
            &NodeConfig::default()
                .being_validator()
                .with_name("mynode2")
                .with_image("myrepo:mysuperimage")
        );
    }
}
