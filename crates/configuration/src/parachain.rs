use std::marker::PhantomData;

use crate::shared::{
    macros::states,
    node::{self, NodeConfig, NodeConfigBuilder},
    types::{AssetLocation, MultiAddress},
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
    id: u16,

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
    bootnodes_addresses: Vec<MultiAddress>,

    /// List of parachain's collators to use.
    collators: Vec<NodeConfig>,
}

impl ParachainConfig {
    pub fn id(&self) -> u16 {
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

    pub fn bootnodes_addresses(&self) -> Vec<&MultiAddress> {
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
    _state: PhantomData<State>,
}

impl ParachainConfigBuilder<Initial> {
    pub fn new() -> ParachainConfigBuilder<Initial> {
        ParachainConfigBuilder {
            config: ParachainConfig {
                id: 0,
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
            _state: PhantomData,
        }
    }
}

impl<A> ParachainConfigBuilder<A> {
    fn transition<B>(config: ParachainConfig) -> ParachainConfigBuilder<B> {
        ParachainConfigBuilder {
            config,
            _state: PhantomData,
        }
    }
}

impl ParachainConfigBuilder<Initial> {
    pub fn with_id(self, id: u16) -> ParachainConfigBuilder<WithId> {
        Self::transition(ParachainConfig { id, ..self.config })
    }
}

impl ParachainConfigBuilder<WithId> {
    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::WithCommand>,
    ) -> ParachainConfigBuilder<WithAtLeastOneCollator> {
        let new_collator = f(NodeConfigBuilder::new()).build();

        Self::transition(ParachainConfig {
            collators: vec![new_collator],
            ..self.config
        })
    }
}

impl ParachainConfigBuilder<WithAtLeastOneCollator> {
    pub fn with_chain(self, chain: &str) -> Self {
        Self::transition(ParachainConfig {
            chain: Some(chain.into()),
            ..self.config
        })
    }

    pub fn with_registration_strategy(self, strategy: RegistrationStrategy) -> Self {
        Self::transition(ParachainConfig {
            registration_strategy: Some(strategy),
            ..self.config
        })
    }

    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self::transition(ParachainConfig {
            initial_balance,
            ..self.config
        })
    }

    pub fn with_genesis_wasm_path(self, location: AssetLocation) -> Self {
        Self::transition(ParachainConfig {
            genesis_wasm_path: Some(location),
            ..self.config
        })
    }

    pub fn with_genesis_wasm_generator(self, command: &str) -> Self {
        Self::transition(ParachainConfig {
            genesis_wasm_generator: Some(command.to_owned()),
            ..self.config
        })
    }

    pub fn with_genesis_state_path(self, location: AssetLocation) -> Self {
        Self::transition(ParachainConfig {
            genesis_state_path: Some(location),
            ..self.config
        })
    }

    pub fn with_genesis_state_generator(self, command: &str) -> Self {
        Self::transition(ParachainConfig {
            genesis_state_generator: Some(command.to_owned()),
            ..self.config
        })
    }

    pub fn with_chain_spec_path(self, location: AssetLocation) -> Self {
        Self::transition(ParachainConfig {
            chain_spec_path: Some(location),
            ..self.config
        })
    }

    pub fn is_cumulus_based(self, choice: bool) -> Self {
        Self::transition(ParachainConfig {
            is_cumulus_based: choice,
            ..self.config
        })
    }

    pub fn with_bootnodes_addresses(self, bootnodes_addresses: Vec<MultiAddress>) -> Self {
        Self::transition(ParachainConfig {
            bootnodes_addresses,
            ..self.config
        })
    }

    pub fn with_collator(
        self,
        f: fn(NodeConfigBuilder<node::Initial>) -> NodeConfigBuilder<node::WithCommand>,
    ) -> Self {
        let new_collator = f(NodeConfigBuilder::new()).build();

        Self::transition(ParachainConfig {
            collators: vec![self.config.collators, vec![new_collator]].concat(),
            ..self.config
        })
    }

    pub fn build(self) -> ParachainConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::ParachainConfigBuilder;

    #[test]
    fn parachain_config_builder_should_build_a_new_parachain_config_correctly() {}
}
