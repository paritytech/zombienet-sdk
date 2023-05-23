use crate::shared::{
    node::NodeConfig,
    types::{AssetLocation, MultiAddress},
};

#[derive(Debug, Clone)]
pub enum RegistrationStrategy {
    InGenesis,
    UsingExtrinsic,
}

/// A parachain configuration, composed of collators and fine-grained configuration options.
#[derive(Debug, Clone)]
pub struct ParachainConfig {
    // Parachain ID to use.
    id: u16,

    /// Chain to use (use None if you are running adder-collator or undying-collator).
    chain: Option<String>,

    /// Registration strategy for the parachain.
    registration_strategy: Option<RegistrationStrategy>,

    /// Parachain balance.
    /// [TODO]: rename to initial_balance ? shouldnt be u128 ?
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
    // [TODO]: do we need node_groups in the sdk?
    // collator_groups?: NodeGroupConfig[];
    // genesis?: JSON | ObjectJSON;
    // [TODO]: should we have default image, resource, command and db snapshot like relaychain ?
}

impl Default for ParachainConfig {
    fn default() -> Self {
        // [TODO]: define the default value for a parachain
        todo!()
    }
}

impl ParachainConfig {
    pub fn with_id(self, id: u16) -> Self {
        Self { id, ..self }
    }

    pub fn with_chain(self, chain: String) -> Self {
        Self {
            chain: Some(chain),
            ..self
        }
    }

    pub fn with_registration_strategy(self, strategy: RegistrationStrategy) -> Self {
        Self {
            registration_strategy: Some(strategy),
            ..self
        }
    }

    pub fn with_initial_balance(self, initial_balance: u128) -> Self {
        Self {
            initial_balance,
            ..self
        }
    }

    pub fn with_genesis_wasm_path(self, location: AssetLocation) -> Self {
        Self {
            genesis_wasm_path: Some(location),
            ..self
        }
    }

    pub fn with_genesis_wasm_generator(self, command: &str) -> Self {
        Self {
            genesis_wasm_generator: Some(command.to_owned()),
            ..self
        }
    }

    pub fn with_genesis_state_path(self, location: AssetLocation) -> Self {
        Self {
            genesis_state_path: Some(location),
            ..self
        }
    }

    pub fn with_genesis_state_generator(self, command: &str) -> Self {
        Self {
            genesis_state_generator: Some(command.to_owned()),
            ..self
        }
    }

    pub fn with_chain_spec_path(self, location: AssetLocation) -> Self {
        Self {
            chain_spec_path: Some(location),
            ..self
        }
    }

    pub fn with_cumulus(self) -> Self {
        Self {
            is_cumulus_based: true,
            ..self
        }
    }

    pub fn with_bootnodes_addresses(self, bootnodes_addresses: Vec<MultiAddress>) -> Self {
        Self {
            bootnodes_addresses,
            ..self
        }
    }

    pub fn with_collator(self, f: fn(NodeConfig) -> NodeConfig) -> Self {
        Self {
            collators: vec![self.collators, vec![f(NodeConfig::default())]].concat(),
            ..self
        }
    }
}
