use std::{cell::RefCell, error::Error, fmt::Display, marker::PhantomData, rc::Rc};

use multiaddr::Multiaddr;
use serde::{ser::SerializeStruct, Deserialize, Serialize};

use crate::{
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{merge_errors, merge_errors_vecs},
        macros::states,
        node::{self, NodeConfig, NodeConfigBuilder},
        resources::{Resources, ResourcesBuilder},
        types::{
            Arg, AssetLocation, Chain, ChainDefaultContext, Command, Image, ValidationContext, U128,
        },
    },
    utils::default_as_true,
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum RegistrationStrategy {
    InGenesis,
    UsingExtrinsic,
}

impl Serialize for RegistrationStrategy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("RegistrationStrategy", 1)?;

        match self {
            Self::InGenesis => state.serialize_field("add_to_genesis", &true)?,
            Self::UsingExtrinsic => state.serialize_field("register_para", &true)?,
        }

        state.end()
    }
}

/// A parachain configuration, composed of collators and fine-grained configuration options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParachainConfig {
    id: u32,
    chain: Option<Chain>,
    #[serde(flatten)]
    registration_strategy: Option<RegistrationStrategy>,
    #[serde(
        skip_serializing_if = "super::utils::is_true",
        default = "default_as_true"
    )]
    onboard_as_parachain: bool,
    #[serde(rename = "balance")]
    initial_balance: U128,
    default_command: Option<Command>,
    default_image: Option<Image>,
    default_resources: Option<Resources>,
    default_db_snapshot: Option<AssetLocation>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    default_args: Vec<Arg>,
    genesis_wasm_path: Option<AssetLocation>,
    genesis_wasm_generator: Option<Command>,
    genesis_state_path: Option<AssetLocation>,
    genesis_state_generator: Option<Command>,
    chain_spec_path: Option<AssetLocation>,
    #[serde(rename = "cumulus_based")]
    is_cumulus_based: bool,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    bootnodes_addresses: Vec<Multiaddr>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    collators: Vec<NodeConfig>,
}

impl ParachainConfig {
    /// The parachain ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The chain name.
    pub fn chain(&self) -> Option<&Chain> {
        self.chain.as_ref()
    }

    /// The registration strategy for the parachain.
    pub fn registration_strategy(&self) -> Option<&RegistrationStrategy> {
        self.registration_strategy.as_ref()
    }

    /// Whether the parachain should be onboarded or stay a parathread
    pub fn onboard_as_parachain(&self) -> bool {
        self.onboard_as_parachain
    }

    /// The initial balance of the parachain account.
    pub fn initial_balance(&self) -> u128 {
        self.initial_balance.0
    }

    /// The default command used for collators.
    pub fn default_command(&self) -> Option<&Command> {
        self.default_command.as_ref()
    }

    /// The default container image used for collators.
    pub fn default_image(&self) -> Option<&Image> {
        self.default_image.as_ref()
    }

    /// The default resources limits used for collators.
    pub fn default_resources(&self) -> Option<&Resources> {
        self.default_resources.as_ref()
    }

    /// The default database snapshot location that will be used for state.
    pub fn default_db_snapshot(&self) -> Option<&AssetLocation> {
        self.default_db_snapshot.as_ref()
    }

    /// The default arguments that will be used to execute the collator command.
    pub fn default_args(&self) -> Vec<&Arg> {
        self.default_args.iter().collect::<Vec<&Arg>>()
    }

    /// The location of a pre-existing genesis WASM runtime blob of the parachain.
    pub fn genesis_wasm_path(&self) -> Option<&AssetLocation> {
        self.genesis_wasm_path.as_ref()
    }

    /// The generator command used to create the genesis WASM runtime blob of the parachain.
    pub fn genesis_wasm_generator(&self) -> Option<&Command> {
        self.genesis_wasm_generator.as_ref()
    }

    /// The location of a pre-existing genesis state of the parachain.
    pub fn genesis_state_path(&self) -> Option<&AssetLocation> {
        self.genesis_state_path.as_ref()
    }

    /// The generator command used to create the genesis state of the parachain.
    pub fn genesis_state_generator(&self) -> Option<&Command> {
        self.genesis_state_generator.as_ref()
    }

    /// The location of a pre-existing chain specification for the parachain.
    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    /// Whether the parachain is based on cumulus.
    pub fn is_cumulus_based(&self) -> bool {
        self.is_cumulus_based
    }

    /// The bootnodes addresses the collators will connect to.
    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect::<Vec<_>>()
    }

    /// The collators of the parachain.
    pub fn collators(&self) -> Vec<&NodeConfig> {
        self.collators.iter().collect::<Vec<_>>()
    }
}

states! {
    Initial,
    WithId,
    WithAtLeastOneCollator
}

/// A parachain configuration builder, used to build a [`ParachainConfig`] declaratively with fields validation.
#[derive(Debug)]
pub struct ParachainConfigBuilder<S> {
    config: ParachainConfig,
    validation_context: Rc<RefCell<ValidationContext>>,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<S>,
}

impl Default for ParachainConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: ParachainConfig {
                id: 100,
                chain: None,
                registration_strategy: Some(RegistrationStrategy::InGenesis),
                onboard_as_parachain: true,
                initial_balance: 2_000_000_000_000.into(),
                default_command: None,
                default_image: None,
                default_resources: None,
                default_db_snapshot: None,
                default_args: vec![],
                genesis_wasm_path: None,
                genesis_wasm_generator: None,
                genesis_state_path: None,
                genesis_state_generator: None,
                chain_spec_path: None,
                is_cumulus_based: true,
                bootnodes_addresses: vec![],
                collators: vec![],
            },
            validation_context: Default::default(),
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> ParachainConfigBuilder<A> {
    fn transition<B>(
        config: ParachainConfig,
        validation_context: Rc<RefCell<ValidationContext>>,
        errors: Vec<anyhow::Error>,
    ) -> ParachainConfigBuilder<B> {
        ParachainConfigBuilder {
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
            default_args: self.config.default_args().into_iter().cloned().collect(),
        }
    }
}

impl ParachainConfigBuilder<Initial> {
    pub fn new(
        validation_context: Rc<RefCell<ValidationContext>>,
    ) -> ParachainConfigBuilder<Initial> {
        Self {
            validation_context,
            ..Self::default()
        }
    }

    /// Set the parachain ID (should be unique).
    // TODO: handle unique validation
    pub fn with_id(self, id: u32) -> ParachainConfigBuilder<WithId> {
        Self::transition(
            ParachainConfig { id, ..self.config },
            self.validation_context,
            self.errors,
        )
    }
}

impl ParachainConfigBuilder<WithId> {
    /// Set the chain name (e.g. rococo-local).
    /// Use [`None`], if you are running adder-collator or undying-collator).
    pub fn with_chain<T>(self, chain: T) -> Self
    where
        T: TryInto<Chain>,
        T::Error: Error + Send + Sync + 'static,
    {
        match chain.try_into() {
            Ok(chain) => Self::transition(
                ParachainConfig {
                    chain: Some(chain),
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

    /// Set the registration strategy for the parachain, could be without registration, using extrinsic or in genesis.
    pub fn with_registration_strategy(self, strategy: RegistrationStrategy) -> Self {
        Self::transition(
            ParachainConfig {
                registration_strategy: Some(strategy),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set whether the parachain should be onboarded or stay a parathread. Default is ```true```.
    pub fn onboard_as_parachain(self, choice: bool) -> Self {
        Self::transition(
            ParachainConfig {
                onboard_as_parachain: choice,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the initial balance of the parachain account.
    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self::transition(
            ParachainConfig {
                initial_balance: initial_balance.into(),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the default command used for collators. Can be overridden.
    pub fn with_default_command<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                ParachainConfig {
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

    /// Set the default container image used for collators. Can be overridden.
    pub fn with_default_image<T>(self, image: T) -> Self
    where
        T: TryInto<Image>,
        T::Error: Error + Send + Sync + 'static,
    {
        match image.try_into() {
            Ok(image) => Self::transition(
                ParachainConfig {
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

    /// Set the default resources limits used for collators. Can be overridden.
    pub fn with_default_resources(self, f: fn(ResourcesBuilder) -> ResourcesBuilder) -> Self {
        match f(ResourcesBuilder::new()).build() {
            Ok(default_resources) => Self::transition(
                ParachainConfig {
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
            ParachainConfig {
                default_db_snapshot: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the default arguments that will be used to execute the collator command. Can be overridden.
    pub fn with_default_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            ParachainConfig {
                default_args: args,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the location of a pre-existing genesis WASM runtime blob of the parachain.
    pub fn with_genesis_wasm_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_wasm_path: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the generator command used to create the genesis WASM runtime blob of the parachain.
    pub fn with_genesis_wasm_generator<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                ParachainConfig {
                    genesis_wasm_generator: Some(command),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(
                    self.errors,
                    FieldError::GenesisWasmGenerator(error.into()).into(),
                ),
            ),
        }
    }

    /// Set the location of a pre-existing genesis state of the parachain.
    pub fn with_genesis_state_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_state_path: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the generator command used to create the genesis state of the parachain.
    pub fn with_genesis_state_generator<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                ParachainConfig {
                    genesis_state_generator: Some(command),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(
                    self.errors,
                    FieldError::GenesisStateGenerator(error.into()).into(),
                ),
            ),
        }
    }

    /// Set the location of a pre-existing chain specification for the parachain.
    pub fn with_chain_spec_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                chain_spec_path: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set whether the parachain is based on cumulus (true in a majority of case, except adder or undying collators).
    pub fn cumulus_based(self, choice: bool) -> Self {
        Self::transition(
            ParachainConfig {
                is_cumulus_based: choice,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the bootnodes addresses the collators will connect to.
    pub fn with_bootnodes_addresses<T>(self, bootnodes_addresses: Vec<T>) -> Self
    where
        T: TryInto<Multiaddr> + Display + Copy,
        T::Error: Error + Send + Sync + 'static,
    {
        let mut addrs = vec![];
        let mut errors = vec![];

        for (index, addr) in bootnodes_addresses.into_iter().enumerate() {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(error) => errors.push(
                    FieldError::BootnodesAddress(index, addr.to_string(), error.into()).into(),
                ),
            }
        }

        Self::transition(
            ParachainConfig {
                bootnodes_addresses: addrs,
                ..self.config
            },
            self.validation_context,
            merge_errors_vecs(self.errors, errors),
        )
    }

    /// Add a new collator using a nested [`NodeConfigBuilder`].
    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> ParachainConfigBuilder<WithAtLeastOneCollator> {
        match f(NodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(collator) => Self::transition(
                ParachainConfig {
                    collators: vec![collator],
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
                        .map(|error| ConfigError::Collator(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }
}

impl ParachainConfigBuilder<WithAtLeastOneCollator> {
    /// Add a new collator using a nested [`NodeConfigBuilder`].
    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match f(NodeConfigBuilder::new(
            ChainDefaultContext::default(),
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(collator) => Self::transition(
                ParachainConfig {
                    collators: [self.config.collators, vec![collator]].concat(),
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
                        .map(|error| ConfigError::Collator(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    /// Seals the builder and returns a [`ParachainConfig`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<ParachainConfig, Vec<anyhow::Error>> {
        if !self.errors.is_empty() {
            return Err(self
                .errors
                .into_iter()
                .map(|error| ConfigError::Parachain(self.config.id, error).into())
                .collect::<Vec<_>>());
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parachain_config_builder_should_succeeds_and_returns_a_new_parachain_config() {
        let parachain_config = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("mychainname")
            .with_registration_strategy(RegistrationStrategy::UsingExtrinsic)
            .onboard_as_parachain(false)
            .with_initial_balance(100_000_042)
            .with_default_image("myrepo:myimage")
            .with_default_command("default_command")
            .with_default_resources(|resources| {
                resources
                    .with_limit_cpu("500M")
                    .with_limit_memory("1G")
                    .with_request_cpu("250M")
            })
            .with_default_db_snapshot("https://www.urltomysnapshot.com/file.tgz")
            .with_default_args(vec![("--arg1", "value1").into(), "--option2".into()])
            .with_genesis_wasm_path("https://www.backupsite.com/my/wasm/file.tgz")
            .with_genesis_wasm_generator("generator_wasm")
            .with_genesis_state_path("./path/to/genesis/state")
            .with_genesis_state_generator("generator_state")
            .with_chain_spec_path("./path/to/chain/spec.json")
            .cumulus_based(false)
            .with_bootnodes_addresses(vec![
                "/ip4/10.41.122.55/tcp/45421",
                "/ip4/51.144.222.10/tcp/2333",
            ])
            .with_collator(|collator| {
                collator
                    .with_name("collator1")
                    .with_command("command1")
                    .bootnode(true)
            })
            .with_collator(|collator| {
                collator
                    .with_name("collator2")
                    .with_command("command2")
                    .validator(true)
            })
            .build()
            .unwrap();

        assert_eq!(parachain_config.id(), 1000);
        assert_eq!(parachain_config.collators().len(), 2);
        let &collator1 = parachain_config.collators().first().unwrap();
        assert_eq!(collator1.name(), "collator1");
        assert_eq!(collator1.command().unwrap().as_str(), "command1");
        assert!(collator1.is_bootnode());
        let &collator2 = parachain_config.collators().last().unwrap();
        assert_eq!(collator2.name(), "collator2");
        assert_eq!(collator2.command().unwrap().as_str(), "command2");
        assert!(collator2.is_validator());
        assert_eq!(parachain_config.chain().unwrap().as_str(), "mychainname");
        assert_eq!(
            parachain_config.registration_strategy().unwrap(),
            &RegistrationStrategy::UsingExtrinsic
        );
        assert!(!parachain_config.onboard_as_parachain());
        assert_eq!(parachain_config.initial_balance(), 100_000_042);
        assert_eq!(
            parachain_config.default_command().unwrap().as_str(),
            "default_command"
        );
        assert_eq!(
            parachain_config.default_image().unwrap().as_str(),
            "myrepo:myimage"
        );
        let default_resources = parachain_config.default_resources().unwrap();
        assert_eq!(default_resources.limit_cpu().unwrap().as_str(), "500M");
        assert_eq!(default_resources.limit_memory().unwrap().as_str(), "1G");
        assert_eq!(default_resources.request_cpu().unwrap().as_str(), "250M");
        assert!(matches!(
            parachain_config.default_db_snapshot().unwrap(),
            AssetLocation::Url(value) if value.as_str() == "https://www.urltomysnapshot.com/file.tgz",
        ));
        assert!(matches!(
            parachain_config.chain_spec_path().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/chain/spec.json"
        ));
        let args: Vec<Arg> = vec![("--arg1", "value1").into(), "--option2".into()];
        assert_eq!(
            parachain_config.default_args(),
            args.iter().collect::<Vec<_>>()
        );
        assert!(matches!(
            parachain_config.genesis_wasm_path().unwrap(),
            AssetLocation::Url(value) if value.as_str() == "https://www.backupsite.com/my/wasm/file.tgz"
        ));
        assert_eq!(
            parachain_config.genesis_wasm_generator().unwrap().as_str(),
            "generator_wasm"
        );
        assert!(matches!(
            parachain_config.genesis_state_path().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/genesis/state"
        ));
        assert_eq!(
            parachain_config.genesis_state_generator().unwrap().as_str(),
            "generator_state"
        );
        assert!(matches!(
            parachain_config.chain_spec_path().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/chain/spec.json"
        ));
        assert!(!parachain_config.is_cumulus_based());
        let bootnodes_addresses: Vec<Multiaddr> = vec![
            "/ip4/10.41.122.55/tcp/45421".try_into().unwrap(),
            "/ip4/51.144.222.10/tcp/2333".try_into().unwrap(),
        ];
        assert_eq!(
            parachain_config.bootnodes_addresses(),
            bootnodes_addresses.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_chain_is_invalid() {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("invalid chain")
            .with_collator(|collator| {
                collator
                    .with_name("collator")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[1000].chain: 'invalid chain' shouldn't contains whitespace"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_default_command_is_invalid() {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("chain")
            .with_default_command("invalid command")
            .with_collator(|collator| {
                collator
                    .with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[1000].default_command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_default_image_is_invalid() {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("chain")
            .with_default_image("invalid image")
            .with_collator(|collator| {
                collator
                    .with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            r"parachain[1000].default_image: 'invalid image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_default_resources_are_invalid()
    {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("chain")
            .with_default_resources(|default_resources| {
                default_resources
                    .with_limit_memory("100m")
                    .with_request_cpu("invalid")
            })
            .with_collator(|collator| {
                collator
                    .with_name("node")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            r"parachain[1000].default_resources.request_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_genesis_wasm_generator_is_invalid(
    ) {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_genesis_wasm_generator("invalid command")
            .with_collator(|collator| {
                collator
                    .with_name("collator")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[2000].genesis_wasm_generator: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_genesis_state_generator_is_invalid(
    ) {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("myparachain")
            .with_genesis_state_generator("invalid command")
            .with_collator(|collator| {
                collator
                    .with_name("collator")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[1000].genesis_state_generator: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_bootnodes_addresses_are_invalid(
    ) {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421", "//10.42.153.10/tcp/43111"])
            .with_collator(|collator| {
                collator
                    .with_name("collator")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[2000].bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "parachain[2000].bootnodes_addresses[1]: '//10.42.153.10/tcp/43111' unknown protocol string: "
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_an_error_if_first_collator_is_invalid() {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("myparachain")
            .with_collator(|collator| {
                collator
                    .with_name("collator")
                    .with_command("invalid command")
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[1000].collators['collator'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn parachain_config_builder_with_at_least_one_collator_should_fails_and_returns_an_error_if_second_collator_is_invalid(
    ) {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_collator(|collator| {
                collator
                    .with_name("collator1")
                    .with_command("command1")
                    .invulnerable(true)
                    .bootnode(true)
            })
            .with_collator(|collator| {
                collator
                    .with_name("collator2")
                    .with_command("invalid command")
                    .with_initial_balance(20000000)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[2000].collators['collator2'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn parachain_config_builder_should_fails_and_returns_multiple_errors_if_multiple_fields_are_invalid(
    ) {
        let errors = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421", "//10.42.153.10/tcp/43111"])
            .with_collator(|collator| {
                collator
                    .with_name("collator1")
                    .with_command("invalid command")
                    .invulnerable(true)
                    .bootnode(true)
                    .with_resources(|resources| {
                        resources
                            .with_limit_cpu("invalid")
                            .with_request_memory("1G")
                    })
            })
            .with_collator(|collator| {
                collator
                    .with_name("collator2")
                    .with_command("command2")
                    .with_image("invalid.image")
                    .with_initial_balance(20000000)
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 5);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[2000].bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "parachain[2000].bootnodes_addresses[1]: '//10.42.153.10/tcp/43111' unknown protocol string: "
        );
        assert_eq!(
            errors.get(2).unwrap().to_string(),
            "parachain[2000].collators['collator1'].command: 'invalid command' shouldn't contains whitespace"
        );
        assert_eq!(
            errors.get(3).unwrap().to_string(),
            r"parachain[2000].collators['collator1'].resources.limit_cpu: 'invalid' doesn't match regex '^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'",
        );
        assert_eq!(
            errors.get(4).unwrap().to_string(),
            "parachain[2000].collators['collator2'].image: 'invalid.image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn onboard_as_parachain_should_default_to_true() {
        let config = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_collator(|collator| collator.with_name("collator"))
            .build()
            .unwrap();

        assert!(config.onboard_as_parachain());
    }
}
