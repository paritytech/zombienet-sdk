use std::{error::Error, marker::PhantomData};

use multiaddr::Multiaddr;

use crate::shared::{
    errors::{ConfigError, FieldError},
    helpers::{merge_errors, merge_errors_vecs},
    macros::states,
    node::{self, NodeConfig, NodeConfigBuilder},
    types::{AssetLocation, Chain, Command, ParaId},
};

#[derive(Debug, Clone, PartialEq)]
pub enum RegistrationStrategy {
    InGenesis,
    UsingExtrinsic,
}

/// A parachain configuration, composed of collators and fine-grained configuration options.
#[derive(Debug, Clone, PartialEq)]
pub struct ParachainConfig {
    // Parachain ID to use.
    id: u32,

    /// Chain to use (use None if you are running adder-collator or undying-collator).
    chain: Option<Chain>,

    /// Registration strategy for the parachain.
    registration_strategy: Option<RegistrationStrategy>,

    /// Parachain balance.
    initial_balance: u128,

    /// Path to WASM runtime.
    genesis_wasm_path: Option<AssetLocation>,

    /// Command to generate the WASM runtime.
    genesis_wasm_generator: Option<Command>,

    /// Path to the gensis `state` file.
    genesis_state_path: Option<AssetLocation>,

    /// Command to generate the genesis `state`.
    genesis_state_generator: Option<Command>,

    /// Use a pre-generated chain specification.
    chain_spec_path: Option<AssetLocation>,

    /// Wether the parachain is based on cumulus (true in a majority of case, except adder or undying collators).
    is_cumulus_based: bool,

    /// List of parachain's bootnodes addresses to use.
    bootnodes_addresses: Vec<Multiaddr>,

    /// List of parachain's collators to use.
    collators: Vec<NodeConfig>,
}

impl ParachainConfig {
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn chain(&self) -> Option<&Chain> {
        self.chain.as_ref()
    }

    pub fn registration_strategy(&self) -> Option<&RegistrationStrategy> {
        self.registration_strategy.as_ref()
    }

    pub fn initial_balance(&self) -> u128 {
        self.initial_balance
    }

    pub fn genesis_wasm_path(&self) -> Option<&AssetLocation> {
        self.genesis_wasm_path.as_ref()
    }

    pub fn genesis_wasm_generator(&self) -> Option<&Command> {
        self.genesis_wasm_generator.as_ref()
    }

    pub fn genesis_state_path(&self) -> Option<&AssetLocation> {
        self.genesis_state_path.as_ref()
    }

    pub fn genesis_state_generator(&self) -> Option<&Command> {
        self.genesis_state_generator.as_ref()
    }

    pub fn chain_spec_path(&self) -> Option<&AssetLocation> {
        self.chain_spec_path.as_ref()
    }

    pub fn is_cumulus_based(&self) -> bool {
        self.is_cumulus_based
    }

    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect::<Vec<_>>()
    }

    pub fn collators(&self) -> Vec<&NodeConfig> {
        self.collators.iter().collect::<Vec<_>>()
    }
}

states! {
    Initial,
    WithId,
    WithAtLeastOneCollator
}

#[derive(Debug)]
pub struct ParachainConfigBuilder<S> {
    config: ParachainConfig,
    errors: Vec<Box<dyn Error>>,
    _state: PhantomData<S>,
}

impl Default for ParachainConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: ParachainConfig {
                id: 100,
                chain: None,
                registration_strategy: Some(RegistrationStrategy::InGenesis),
                initial_balance: 2_000_000_000_000,
                genesis_wasm_path: None,
                genesis_wasm_generator: None,
                genesis_state_path: None,
                genesis_state_generator: None,
                chain_spec_path: None,
                is_cumulus_based: true,
                bootnodes_addresses: vec![],
                collators: vec![],
            },
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> ParachainConfigBuilder<A> {
    fn transition<B>(
        config: ParachainConfig,
        errors: Vec<Box<dyn Error>>,
    ) -> ParachainConfigBuilder<B> {
        ParachainConfigBuilder {
            config,
            errors,
            _state: PhantomData,
        }
    }
}

impl ParachainConfigBuilder<Initial> {
    pub fn new() -> ParachainConfigBuilder<Initial> {
        Self::default()
    }

    pub fn with_id(self, id: u32) -> ParachainConfigBuilder<WithId> {
        Self::transition(ParachainConfig { id, ..self.config }, self.errors)
    }
}

impl ParachainConfigBuilder<WithId> {
    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> ParachainConfigBuilder<WithAtLeastOneCollator> {
        match f(NodeConfigBuilder::new(None)).build() {
            Ok(collator) => Self::transition(
                ParachainConfig {
                    collators: vec![collator],
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
                        .map(|error| ConfigError::Collator(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }
}

impl ParachainConfigBuilder<WithAtLeastOneCollator> {
    pub fn with_chain<T>(self, chain: T) -> Self
    where
        T: TryInto<Chain>,
        T::Error: Error + 'static,
    {
        match chain.try_into() {
            Ok(chain) => Self::transition(
                ParachainConfig {
                    chain: Some(chain),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::Chain(error).into()),
            ),
        }
    }

    pub fn with_registration_strategy(self, strategy: RegistrationStrategy) -> Self {
        Self::transition(
            ParachainConfig {
                registration_strategy: Some(strategy),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self::transition(
            ParachainConfig {
                initial_balance,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_genesis_wasm_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_wasm_path: Some(location.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_genesis_wasm_generator<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                ParachainConfig {
                    genesis_wasm_generator: Some(command),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::GenesisWasmGenerator(error).into()),
            ),
        }
    }

    pub fn with_genesis_state_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_state_path: Some(location.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_genesis_state_generator<T>(self, command: T) -> Self
    where
        T: TryInto<Command>,
        T::Error: Error + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                ParachainConfig {
                    genesis_state_generator: Some(command),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::GenesisStateGenerator(error).into()),
            ),
        }
    }

    pub fn with_chain_spec_path(self, location: impl Into<AssetLocation>) -> Self {
        Self::transition(
            ParachainConfig {
                chain_spec_path: Some(location.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn cumulus_based(self, choice: bool) -> Self {
        Self::transition(
            ParachainConfig {
                is_cumulus_based: choice,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_bootnodes_addresses<T>(self, bootnodes_addresses: Vec<T>) -> Self
    where
        T: TryInto<Multiaddr> + ToString + Copy,
        T::Error: Error + 'static,
    {
        let mut addrs = vec![];
        let mut errors = vec![];

        for (index, addr) in bootnodes_addresses.into_iter().enumerate() {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(error) => {
                    errors.push(FieldError::BootnodesAddress(index, addr.to_string(), error).into())
                },
            }
        }

        Self::transition(
            ParachainConfig {
                bootnodes_addresses: addrs,
                ..self.config
            },
            merge_errors_vecs(self.errors, errors),
        )
    }

    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        match f(NodeConfigBuilder::new(None)).build() {
            Ok(collator) => Self::transition(
                ParachainConfig {
                    collators: vec![self.config.collators, vec![collator]].concat(),
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
                        .map(|error| ConfigError::Collator(name.clone(), error).into())
                        .collect::<Vec<_>>(),
                ),
            ),
        }
    }

    pub fn build(self) -> Result<ParachainConfig, (ParaId, Vec<Box<dyn Error>>)> {
        if !self.errors.is_empty() {
            return Err((self.config.id, self.errors));
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parachain_config_builder_should_build_a_new_parachain_config_correctly() {
        let parachain_config = ParachainConfigBuilder::new()
            .with_id(1000)
            .with_collator(|collator1| {
                collator1
                    .with_name("collator1")
                    .with_command("command1")
                    .bootnode(true)
            })
            .with_collator(|collator2| {
                collator2
                    .with_name("collator2")
                    .with_command("command2")
                    .validator(true)
            })
            .with_chain("mychainname")
            .with_registration_strategy(RegistrationStrategy::UsingExtrinsic)
            .with_initial_balance(100_000_042)
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
        assert!(collator2.is_validator(), "node2");
        assert_eq!(parachain_config.chain().unwrap().as_str(), "mychainname");
        assert_eq!(
            parachain_config.registration_strategy().unwrap(),
            &RegistrationStrategy::UsingExtrinsic
        );
        assert_eq!(parachain_config.initial_balance(), 100_000_042);
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
}
