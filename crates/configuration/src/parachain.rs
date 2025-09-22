use std::{cell::RefCell, error::Error, fmt::Display, marker::PhantomData, rc::Rc};

use anyhow::anyhow;
use multiaddr::Multiaddr;
use serde::{
    de::{self, Visitor},
    ser::SerializeStruct,
    Deserialize, Serialize,
};

use crate::{
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{generate_unique_para_id, merge_errors, merge_errors_vecs},
        node::{self, NodeConfig, NodeConfigBuilder},
        resources::{Resources, ResourcesBuilder},
        types::{
            Arg, AssetLocation, Chain, ChainDefaultContext, Command, Image, ValidationContext, U128,
        },
    },
    types::CommandWithCustomArgs,
    utils::{default_as_false, default_as_true, default_initial_balance, is_false},
};

/// The registration strategy that will be used for the parachain.
#[derive(Debug, Clone, PartialEq)]
pub enum RegistrationStrategy {
    /// The parachain will be added to the genesis before spawning.
    InGenesis,
    /// The parachain will be registered using an extrinsic after spawning.
    UsingExtrinsic,
    /// The parachaing will not be registered and the user can doit after spawning manually.
    Manual,
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
            Self::Manual => {
                state.serialize_field("add_to_genesis", &false)?;
                state.serialize_field("register_para", &false)?;
            },
        }

        state.end()
    }
}

struct RegistrationStrategyVisitor;

impl<'de> Visitor<'de> for RegistrationStrategyVisitor {
    type Value = RegistrationStrategy;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("struct RegistrationStrategy")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut add_to_genesis = false;
        let mut register_para = false;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "addToGenesis" | "add_to_genesis" => add_to_genesis = map.next_value()?,
                "registerPara" | "register_para" => register_para = map.next_value()?,
                _ => {
                    return Err(de::Error::unknown_field(
                        &key,
                        &["add_to_genesis", "register_para"],
                    ))
                },
            }
        }

        match (add_to_genesis, register_para) {
            (true, false) => Ok(RegistrationStrategy::InGenesis),
            (false, true) => Ok(RegistrationStrategy::UsingExtrinsic),
            _ => Err(de::Error::missing_field("add_to_genesis or register_para")),
        }
    }
}

impl<'de> Deserialize<'de> for RegistrationStrategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_struct(
            "RegistrationStrategy",
            &["add_to_genesis", "register_para"],
            RegistrationStrategyVisitor,
        )
    }
}

/// A parachain configuration, composed of collators and fine-grained configuration options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParachainConfig {
    id: u32,
    #[serde(skip)]
    // unique_id is internally used to allow multiple parachains with the same id
    // BUT, only one of them could be register automatically at spawn
    unique_id: String,
    chain: Option<Chain>,
    #[serde(flatten)]
    registration_strategy: Option<RegistrationStrategy>,
    #[serde(
        skip_serializing_if = "super::utils::is_true",
        default = "default_as_true"
    )]
    onboard_as_parachain: bool,
    #[serde(rename = "balance", default = "default_initial_balance")]
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
    genesis_state_generator: Option<CommandWithCustomArgs>,
    chain_spec_path: Option<AssetLocation>,
    // Path or url to override the runtime (:code) in the chain-spec
    wasm_override: Option<AssetLocation>,
    // Full _template_ command, will be rendered using [tera]
    // and executed for generate the chain-spec.
    // available tokens {{chainName}} / {{disableBootnodes}}
    chain_spec_command: Option<String>,
    // Does the chain_spec_command needs to be run locally
    #[serde(skip_serializing_if = "is_false", default)]
    chain_spec_command_is_local: bool,
    #[serde(rename = "cumulus_based", default = "default_as_true")]
    is_cumulus_based: bool,
    #[serde(rename = "evm_based", default = "default_as_false")]
    is_evm_based: bool,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    bootnodes_addresses: Vec<Multiaddr>,
    #[serde(skip_serializing_if = "is_false", default)]
    no_default_bootnodes: bool,
    #[serde(rename = "genesis", skip_serializing_if = "Option::is_none")]
    genesis_overrides: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    pub(crate) collators: Vec<NodeConfig>,
    // Single collator config, added for backward compatibility
    // with `toml` networks definitions from v1.
    // This field can only be set loading an old `toml` definition
    // with `[parachain.collator]` key.
    // NOTE: if the file also contains multiple collators defined in
    // `[[parachain.collators]], the single configuration will be added to the bottom.
    pub(crate) collator: Option<NodeConfig>,
}

impl ParachainConfig {
    /// The parachain ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The parachain unique ID.
    pub fn unique_id(&self) -> &str {
        &self.unique_id
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
    pub fn genesis_state_generator(&self) -> Option<&CommandWithCustomArgs> {
        self.genesis_state_generator.as_ref()
    }

    /// The genesis overrides as a JSON value.
    pub fn genesis_overrides(&self) -> Option<&serde_json::Value> {
        self.genesis_overrides.as_ref()
    }

    /// The location of a pre-existing chain specification for the parachain.
    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    /// The full _template_ command to genera the chain-spec
    pub fn chain_spec_command(&self) -> Option<&str> {
        self.chain_spec_command.as_deref()
    }

    /// Does the chain_spec_command needs to be run locally
    pub fn chain_spec_command_is_local(&self) -> bool {
        self.chain_spec_command_is_local
    }

    /// Whether the parachain is based on cumulus.
    pub fn is_cumulus_based(&self) -> bool {
        self.is_cumulus_based
    }

    /// Whether the parachain is evm based (e.g frontier).
    pub fn is_evm_based(&self) -> bool {
        self.is_evm_based
    }

    /// The bootnodes addresses the collators will connect to.
    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect::<Vec<_>>()
    }

    /// Whether to not automatically assign a bootnode role if none of the nodes are marked
    /// as bootnodes.
    pub fn no_default_bootnodes(&self) -> bool {
        self.no_default_bootnodes
    }

    /// The collators of the parachain.
    pub fn collators(&self) -> Vec<&NodeConfig> {
        let mut cols = self.collators.iter().collect::<Vec<_>>();
        if let Some(col) = self.collator.as_ref() {
            cols.push(col);
        }
        cols
    }

    /// The location of a wasm runtime to override in the chain-spec.
    pub fn wasm_override(&self) -> Option<&AssetLocation> {
        self.wasm_override.as_ref()
    }
}

pub mod states {
    use crate::shared::macros::states;

    states! {
        Initial,
        WithId,
        WithAtLeastOneCollator
    }

    states! {
        Bootstrap,
        Running
    }

    pub trait Context {}
    impl Context for Bootstrap {}
    impl Context for Running {}
}

use states::{Bootstrap, Context, Initial, Running, WithAtLeastOneCollator, WithId};
/// A parachain configuration builder, used to build a [`ParachainConfig`] declaratively with fields validation.
pub struct ParachainConfigBuilder<S, C> {
    config: ParachainConfig,
    validation_context: Rc<RefCell<ValidationContext>>,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<S>,
    _context: PhantomData<C>,
}

impl<C: Context> Default for ParachainConfigBuilder<Initial, C> {
    fn default() -> Self {
        Self {
            config: ParachainConfig {
                id: 100,
                unique_id: String::from("100"),
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
                genesis_overrides: None,
                chain_spec_path: None,
                chain_spec_command: None,
                wasm_override: None,
                chain_spec_command_is_local: false, // remote by default
                is_cumulus_based: true,
                is_evm_based: false,
                bootnodes_addresses: vec![],
                no_default_bootnodes: false,
                collators: vec![],
                collator: None,
            },
            validation_context: Default::default(),
            errors: vec![],
            _state: PhantomData,
            _context: PhantomData,
        }
    }
}

impl<A, C> ParachainConfigBuilder<A, C> {
    fn transition<B>(
        config: ParachainConfig,
        validation_context: Rc<RefCell<ValidationContext>>,
        errors: Vec<anyhow::Error>,
    ) -> ParachainConfigBuilder<B, C> {
        ParachainConfigBuilder {
            config,
            validation_context,
            errors,
            _state: PhantomData,
            _context: PhantomData,
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

    fn create_node_builder<F>(&self, f: F) -> NodeConfigBuilder<node::Buildable>
    where
        F: FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    {
        f(NodeConfigBuilder::new(
            self.default_chain_context(),
            self.validation_context.clone(),
        ))
    }
}

impl ParachainConfigBuilder<Initial, Bootstrap> {
    /// Instantiate a new builder that can be used to build a [`ParachainConfig`] during the bootstrap phase.
    pub fn new(
        validation_context: Rc<RefCell<ValidationContext>>,
    ) -> ParachainConfigBuilder<Initial, Bootstrap> {
        Self {
            validation_context,
            ..Self::default()
        }
    }
}

impl ParachainConfigBuilder<WithId, Bootstrap> {
    /// Set the registration strategy for the parachain, could be Manual (no registered by zombienet) or automatic
    /// using an extrinsic or in genesis.
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
}

impl ParachainConfigBuilder<WithId, Running> {
    /// Set the registration strategy for the parachain, could be Manual (no registered by zombienet) or automatic
    /// Using an extrinsic. Genesis option is not allowed in `Running` context.
    pub fn with_registration_strategy(self, strategy: RegistrationStrategy) -> Self {
        match strategy {
            RegistrationStrategy::InGenesis => Self::transition(
                self.config,
                self.validation_context,
                merge_errors(
                    self.errors,
                    FieldError::RegistrationStrategy(anyhow!(
                        "Can be set to InGenesis in Running context"
                    ))
                    .into(),
                ),
            ),
            RegistrationStrategy::Manual | RegistrationStrategy::UsingExtrinsic => {
                Self::transition(
                    ParachainConfig {
                        registration_strategy: Some(strategy),
                        ..self.config
                    },
                    self.validation_context,
                    self.errors,
                )
            },
        }
    }
}

impl ParachainConfigBuilder<Initial, Running> {
    /// Start a new builder in the context of a running network
    pub fn new_with_running(
        validation_context: Rc<RefCell<ValidationContext>>,
    ) -> ParachainConfigBuilder<Initial, Running> {
        let mut builder = Self {
            validation_context,
            ..Self::default()
        };

        // override the registration strategy
        builder.config.registration_strategy = Some(RegistrationStrategy::UsingExtrinsic);
        builder
    }
}

impl<C: Context> ParachainConfigBuilder<Initial, C> {
    /// Set the parachain ID and the unique_id (with the suffix `<para_id>-x` if the id is already used)
    pub fn with_id(self, id: u32) -> ParachainConfigBuilder<WithId, C> {
        let unique_id = generate_unique_para_id(id, self.validation_context.clone());
        Self::transition(
            ParachainConfig {
                id,
                unique_id,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }
}

impl<C: Context> ParachainConfigBuilder<WithId, C> {
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
    pub fn with_default_resources(
        self,
        f: impl FnOnce(ResourcesBuilder) -> ResourcesBuilder,
    ) -> Self {
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
        T: TryInto<CommandWithCustomArgs>,
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

    /// Set the genesis overrides as a JSON object.
    pub fn with_genesis_overrides(self, genesis_overrides: impl Into<serde_json::Value>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_overrides: Some(genesis_overrides.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
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

    /// Set the chain-spec command _template_ for the relay chain.
    pub fn with_chain_spec_command(self, cmd_template: impl Into<String>) -> Self {
        Self::transition(
            ParachainConfig {
                chain_spec_command: Some(cmd_template.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set the location of a wasm to override the chain-spec.
    pub fn with_wasm_override(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                wasm_override: Some(location.into()),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Set if the chain-spec command needs to be run locally or not (false by default)
    pub fn chain_spec_command_is_local(self, choice: bool) -> Self {
        Self::transition(
            ParachainConfig {
                chain_spec_command_is_local: choice,
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

    /// Set whether the parachain is evm based (e.g frontier /evm template)
    pub fn evm_based(self, choice: bool) -> Self {
        Self::transition(
            ParachainConfig {
                is_evm_based: choice,
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

    /// Do not assign a bootnode role automatically if no nodes are marked as bootnodes.
    pub fn without_default_bootnodes(self) -> Self {
        Self::transition(
            ParachainConfig {
                no_default_bootnodes: true,
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Add a new collator using a nested [`NodeConfigBuilder`].
    pub fn with_collator(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> ParachainConfigBuilder<WithAtLeastOneCollator, C> {
        match self.create_node_builder(f).validator(true).build() {
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

    /// Add a new full node using a nested [`NodeConfigBuilder`].
    /// The node will be configured as a full node (non-validator).
    pub fn with_fullnode(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> ParachainConfigBuilder<WithAtLeastOneCollator, C> {
        match self.create_node_builder(f).validator(false).build() {
            Ok(node) => Self::transition(
                ParachainConfig {
                    collators: vec![node],
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

    /// Add a new node using a nested [`NodeConfigBuilder`].
    ///
    /// **Deprecated**: Use [`with_collator`] for collator nodes or [`with_fullnode`] for full nodes instead.
    #[deprecated(
        since = "0.4.0",
        note = "Use `with_collator()` for collator nodes or `with_fullnode()` for full nodes instead"
    )]
    pub fn with_node(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> ParachainConfigBuilder<WithAtLeastOneCollator, C> {
        match self.create_node_builder(f).build() {
            Ok(node) => Self::transition(
                ParachainConfig {
                    collators: vec![node],
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

impl<C: Context> ParachainConfigBuilder<WithAtLeastOneCollator, C> {
    /// Add a new collator using a nested [`NodeConfigBuilder`].
    pub fn with_collator(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match self.create_node_builder(f).validator(true).build() {
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

    /// Add a new full node using a nested [`NodeConfigBuilder`].
    /// The node will be configured as a full node (non-validator).
    pub fn with_fullnode(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match self.create_node_builder(f).validator(false).build() {
            Ok(node) => Self::transition(
                ParachainConfig {
                    collators: [self.config.collators, vec![node]].concat(),
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

    /// Add a new node using a nested [`NodeConfigBuilder`].
    ///
    /// **Deprecated**: Use [`with_collator`] for collator nodes or [`with_fullnode`] for full nodes instead.
    #[deprecated(
        since = "0.4.0",
        note = "Use `with_collator()` for collator nodes or `with_fullnode()` for full nodes instead"
    )]
    pub fn with_node(
        self,
        f: impl FnOnce(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match self.create_node_builder(f).build() {
            Ok(node) => Self::transition(
                ParachainConfig {
                    collators: [self.config.collators, vec![node]].concat(),
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
    use crate::NetworkConfig;

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
            .with_genesis_state_generator(
                "undying-collator export-genesis-state --pov-size=10000 --pvf-complexity=1",
            )
            .with_chain_spec_path("./path/to/chain/spec.json")
            .with_wasm_override("./path/to/override/runtime.wasm")
            .cumulus_based(false)
            .evm_based(false)
            .with_bootnodes_addresses(vec![
                "/ip4/10.41.122.55/tcp/45421",
                "/ip4/51.144.222.10/tcp/2333",
            ])
            .without_default_bootnodes()
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
        assert!(matches!(
            parachain_config.wasm_override().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/override/runtime.wasm"
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
            parachain_config
                .genesis_state_generator()
                .unwrap()
                .cmd()
                .as_str(),
            "undying-collator"
        );

        assert_eq!(
            parachain_config.genesis_state_generator().unwrap().args(),
            &vec![
                "export-genesis-state".into(),
                ("--pov-size", "10000").into(),
                ("--pvf-complexity", "1").into()
            ]
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
        assert!(parachain_config.no_default_bootnodes());
        assert_eq!(
            parachain_config.bootnodes_addresses(),
            bootnodes_addresses.iter().collect::<Vec<_>>()
        );
        assert!(!parachain_config.is_evm_based());
    }

    #[test]
    fn parachain_config_builder_should_works_when_genesis_state_generator_contains_args() {
        let parachain_config = ParachainConfigBuilder::new(Default::default())
            .with_id(1000)
            .with_chain("myparachain")
            .with_genesis_state_generator("generator_state --simple-flag --flag=value")
            .with_collator(|collator| {
                collator
                    .with_name("collator")
                    .with_command("command")
                    .validator(true)
            })
            .build()
            .unwrap();

        assert_eq!(
            parachain_config
                .genesis_state_generator()
                .unwrap()
                .cmd()
                .as_str(),
            "generator_state"
        );

        assert_eq!(
            parachain_config
                .genesis_state_generator()
                .unwrap()
                .args()
                .len(),
            2
        );

        let args = parachain_config.genesis_state_generator().unwrap().args();

        assert_eq!(
            args,
            &vec![
                Arg::Flag("--simple-flag".into()),
                Arg::Option("--flag".into(), "value".into())
            ]
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
            "parachain[2000].genesis_wasm_generator: 'invalid command' shouldn't contains whitespace"
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
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
            errors.first().unwrap().to_string(),
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
    fn import_toml_registration_strategy_should_deserialize() {
        let load_from_toml =
            NetworkConfig::load_from_toml("./testing/snapshots/0001-big-network.toml").unwrap();

        for parachain in load_from_toml.parachains().iter() {
            if parachain.id() == 1000 {
                assert_eq!(
                    parachain.registration_strategy(),
                    Some(&RegistrationStrategy::UsingExtrinsic)
                );
            }
            if parachain.id() == 2000 {
                assert_eq!(
                    parachain.registration_strategy(),
                    Some(&RegistrationStrategy::InGenesis)
                );
            }
        }

        let load_from_toml_small = NetworkConfig::load_from_toml(
            "./testing/snapshots/0003-small-network_w_parachain.toml",
        )
        .unwrap();

        let parachain = load_from_toml_small.parachains()[0];
        let parachain_evm = load_from_toml_small.parachains()[1];

        assert_eq!(parachain.registration_strategy(), None);
        assert!(!parachain.is_evm_based());
        assert_eq!(parachain.collators().len(), 1);
        assert!(parachain_evm.is_evm_based());
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

    #[test]
    fn evm_based_default_to_false() {
        let config = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_collator(|collator| collator.with_name("collator"))
            .build()
            .unwrap();

        assert!(!config.is_evm_based());
    }

    #[test]
    fn evm_based() {
        let config = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .evm_based(true)
            .with_collator(|collator| collator.with_name("collator"))
            .build()
            .unwrap();

        assert!(config.is_evm_based());
    }

    #[test]
    fn build_config_in_running_context() {
        let config = ParachainConfigBuilder::new_with_running(Default::default())
            .with_id(2000)
            .with_chain("myparachain")
            .with_collator(|collator| collator.with_name("collator"))
            .build()
            .unwrap();

        assert_eq!(
            config.registration_strategy(),
            Some(&RegistrationStrategy::UsingExtrinsic)
        );
    }

    #[test]
    fn parachain_config_builder_should_works_with_chain_spec_command() {
        const CMD_TPL: &str = "./bin/chain-spec-generator {% raw %} {{chainName}} {% endraw %}";
        let config = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("some-chain")
            .with_default_image("myrepo:myimage")
            .with_default_command("default_command")
            .with_chain_spec_command(CMD_TPL)
            .with_collator(|collator| collator.with_name("collator"))
            .build()
            .unwrap();

        assert_eq!(config.chain_spec_command(), Some(CMD_TPL));
        assert!(!config.chain_spec_command_is_local());
    }

    #[test]
    fn parachain_config_builder_should_works_with_chain_spec_command_and_local() {
        const CMD_TPL: &str = "./bin/chain-spec-generator {% raw %} {{chainName}} {% endraw %}";
        let config = ParachainConfigBuilder::new(Default::default())
            .with_id(2000)
            .with_chain("some-chain")
            .with_default_image("myrepo:myimage")
            .with_default_command("default_command")
            .with_chain_spec_command(CMD_TPL)
            .chain_spec_command_is_local(true)
            .with_collator(|collator| collator.with_name("collator"))
            .build()
            .unwrap();

        assert_eq!(config.chain_spec_command(), Some(CMD_TPL));
        assert!(config.chain_spec_command_is_local());
    }
}
