use crate::shared::{
    node::NodeConfig,
    types::{Command, MultiAddress, Path},
};

/// A parachain configuration, composed of collators and fine-grained configuration options.
#[derive(Clone)]
pub struct ParachainConfig {
    // Parachain ID to use.
    id: u16,

    /// Chain to use (use None if you are running adder-collator or undying-collator).
    chain: Option<String>,

    /// Wether to add the parachain to the genesis (chain specification) file.
    has_to_be_added_to_genesis: bool,

    /// Wether to register this parachain (via genesis or extrinsic).
    /// [TODO]: is the "via genesis" part of the comment needed given the above option add to genesis ?
    has_to_be_registered: bool,

    /// Parachain balance.
    /// [TODO]: rename to initial_balance ? shouldnt be u128 ?
    balance: u64,

    /// Path to WASM runtime.
    genesis_wasm_path: Option<Path>,

    /// Command to generate the WASM runtime.
    genesis_wasm_generator: Option<Command>,

    /// Path to the gensis `state` file.
    genesis_state_path: Option<Path>,

    /// Command to generate the genesis `state`.
    genesis_state_generator: Option<Command>,

    /// Use a pre-generated chain specification.
    chain_spec_path: Option<Path>,

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

    pub fn with_genesis_addition(self) -> Self {
        Self {
            has_to_be_added_to_genesis: true,
            ..self
        }
    }

    pub fn with_registration(self) -> Self {
        Self {
            has_to_be_registered: true,
            ..self
        }
    }

    pub fn with_balance(self, balance: u64) -> Self {
        Self { balance, ..self }
    }

    pub fn with_genesis_wasm_path(self, path: Path) -> Self {
        Self {
            genesis_wasm_path: Some(path),
            ..self
        }
    }

    pub fn with_genesis_wasm_generator(self, command: Command) -> Self {
        Self {
            genesis_wasm_generator: Some(command),
            ..self
        }
    }

    pub fn with_genesis_state_path(self, path: Path) -> Self {
        Self {
            genesis_state_path: Some(path),
            ..self
        }
    }

    pub fn with_genesis_state_generator(self, command: Command) -> Self {
        Self {
            genesis_state_generator: Some(command),
            ..self
        }
    }

    pub fn with_chain_spec_path(self, path: Path) -> Self {
        Self {
            chain_spec_path: Some(path),
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
