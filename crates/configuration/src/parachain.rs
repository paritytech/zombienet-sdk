use std::marker::PhantomData;

use multiaddr::Multiaddr;

use crate::shared::{
    macros::states,
    node::{self, NodeConfig, NodeConfigBuilder},
    types::AssetLocation,
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
    chain: Option<String>,

    /// Registration strategy for the parachain.
    registration_strategy: Option<RegistrationStrategy>,

    /// Parachain balance.
    initial_balance: u128,

    /// Path to WASM runtime.
    genesis_wasm_path: Option<AssetLocation>,

    /// Command to generate the WASM runtime.
    genesis_wasm_generator: Option<String>,

    /// Path to the gensis `state` file.
    genesis_state_path: Option<AssetLocation>,

    /// Command to generate the genesis `state`.
    genesis_state_generator: Option<String>,

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

    pub fn chain(&self) -> Option<&str> {
        self.chain.as_deref()
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

    pub fn genesis_wasm_generator(&self) -> Option<&str> {
        self.genesis_wasm_generator.as_deref()
    }

    pub fn genesis_state_path(&self) -> Option<&AssetLocation> {
        self.genesis_state_path.as_ref()
    }

    pub fn genesis_state_generator(&self) -> Option<&str> {
        self.genesis_state_generator.as_deref()
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
pub struct ParachainConfigBuilder<State> {
    config: ParachainConfig,
    errors: Vec<String>,
    _state: PhantomData<State>,
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
    fn transition<B>(config: ParachainConfig, errors: Vec<String>) -> ParachainConfigBuilder<B> {
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
        let new_collator = f(NodeConfigBuilder::new(None)).build();

        Self::transition(
            ParachainConfig {
                collators: vec![new_collator],
                ..self.config
            },
            self.errors,
        )
    }
}

impl ParachainConfigBuilder<WithAtLeastOneCollator> {
    pub fn with_chain(self, chain: impl Into<String>) -> Self {
        Self::transition(
            ParachainConfig {
                chain: Some(chain.into()),
                ..self.config
            },
            self.errors,
        )
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

    pub fn with_genesis_wasm_path(self, location: impl TryInto<AssetLocation>) -> Self {
        match location.try_into() {
            Ok(location) => Self::transition(
                ParachainConfig {
                    genesis_wasm_path: Some(location),
                    ..self.config
                },
                self.errors,
            ),
            Err(_) => Self::transition(
                ParachainConfig {
                    genesis_wasm_path: None,
                    ..self.config
                },
                vec![
                    self.errors,
                    vec![format!(
                        "genesis_wasm_path: unable to convert into AssetLocation"
                    )],
                ]
                .concat(),
            ),
        }
    }

    pub fn with_genesis_wasm_generator(self, command: impl Into<String>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_wasm_generator: Some(command.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_genesis_state_path(self, location: impl TryInto<AssetLocation>) -> Self {
        match location.try_into() {
            Ok(location) => Self::transition(
                ParachainConfig {
                    genesis_state_path: Some(location),
                    ..self.config
                },
                self.errors,
            ),
            Err(_) => Self::transition(
                ParachainConfig {
                    genesis_state_path: None,
                    ..self.config
                },
                vec![
                    self.errors,
                    vec![format!(
                        "genesis_state_path: unable to convert into AssetLocation"
                    )],
                ]
                .concat(),
            ),
        }
    }

    pub fn with_genesis_state_generator(self, command: impl Into<String>) -> Self {
        Self::transition(
            ParachainConfig {
                genesis_state_generator: Some(command.into()),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_chain_spec_path(self, location: impl TryInto<AssetLocation>) -> Self {
        match location.try_into() {
            Ok(location) => Self::transition(
                ParachainConfig {
                    chain_spec_path: Some(location),
                    ..self.config
                },
                self.errors,
            ),
            Err(_) => Self::transition(
                ParachainConfig {
                    chain_spec_path: None,
                    ..self.config
                },
                vec![
                    self.errors,
                    vec![format!(
                        "chain_spec_path: unable to convert into AssetLocation"
                    )],
                ]
                .concat(),
            ),
        }
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

    pub fn with_bootnodes_addresses(
        self,
        bootnodes_addresses: Vec<impl TryInto<Multiaddr>>,
    ) -> Self {
        let mut addrs = vec![];
        let mut errors = vec![];

        for addr in bootnodes_addresses {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(_) => errors.push("error multiaddr".to_string()),
            }
        }

        Self::transition(
            ParachainConfig {
                bootnodes_addresses: addrs,
                ..self.config
            },
            vec![self.errors, errors].concat(),
        )
    }

    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::Buildable>,
    ) -> Self {
        let new_collator = f(NodeConfigBuilder::new(None)).build();

        Self::transition(
            ParachainConfig {
                collators: vec![self.config.collators, vec![new_collator]].concat(),
                ..self.config
            },
            self.errors,
        )
    }

    pub fn build(self) -> ParachainConfig {
        self.config
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
            .with_genesis_wasm_generator("my wasm generator command")
            .with_genesis_state_path("./path/to/genesis/state")
            .with_genesis_state_generator("my state generator command")
            .with_chain_spec_path("./path/to/chain/spec.json")
            .cumulus_based(false)
            .with_bootnodes_addresses(vec![
                "/ip4/10.41.122.55/tcp/45421",
                "/ip4/51.144.222.10/tcp/2333",
            ])
            .build();

        assert_eq!(parachain_config.id(), 1000);
        assert_eq!(parachain_config.collators().len(), 2);
        let &collator1 = parachain_config.collators().first().unwrap();
        assert_eq!(collator1.name(), "collator1");
        assert_eq!(collator1.command().unwrap(), "command1");
        assert!(collator1.is_bootnode());
        let &collator2 = parachain_config.collators().last().unwrap();
        assert_eq!(collator2.name(), "collator2");
        assert_eq!(collator2.command().unwrap(), "command2");
        assert!(collator2.is_validator(), "node2");
        assert_eq!(parachain_config.chain().unwrap(), "mychainname");
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
            parachain_config.genesis_wasm_generator().unwrap(),
            "my wasm generator command"
        );
        assert!(matches!(
            parachain_config.genesis_state_path().unwrap(),
            AssetLocation::FilePath(value) if value.to_str().unwrap() == "./path/to/genesis/state"
        ));
        assert_eq!(
            parachain_config.genesis_state_generator().unwrap(),
            "my state generator command"
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
