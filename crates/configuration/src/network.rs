use crate::{
    shared::types::{Duration, IpAddress, MultiAddress},
    HrmpChannelConfig, ParachainConfig, RelaychainConfig,
};

/// Global settings applied to an entire network.
pub struct GlobalSettings {
    /// Whether we should spawn a dedicated bootnode for each chain.
    spawn_bootnode: bool,

    /// External bootnode address.
    /// [TODO]: is it a default overriden by node config, maybe an option ?
    bootnode_address: MultiAddress,

    /// Global spawn timeout in seconds.
    network_spawn_timeout: Duration,

    /// Individual node spawn timeout.
    node_spawn_timeout: Duration,

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    local_ip: Option<IpAddress>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        // [TODO]: define the default value for global settings
        todo!()
    }
}

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
pub struct NetworkConfig {
    /// The global settings applied to the network.
    global_settings: GlobalSettings,

    /// Relaychain configuration.
    relaychain: RelaychainConfig,

    /// Parachains configurations.
    parachains: Vec<ParachainConfig>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<HrmpChannelConfig>,

    // [TODO]: what does it represents ?
    seed: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        // [TODO]: define the default value for a network
        todo!()
    }
}

impl NetworkConfig {
    pub fn new() -> NetworkConfig {
        Self::default()
    }

    pub fn with_global_settings(self, f: fn(GlobalSettings) -> GlobalSettings) -> Self {
        Self {
            global_settings: f(self.global_settings),
            ..self
        }
    }

    pub fn with_relaychain(self, f: fn(RelaychainConfig) -> RelaychainConfig) -> Self {
        Self {
            relaychain: f(self.relaychain),
            ..self
        }
    }

    pub fn with_parachain(self, f: fn(ParachainConfig) -> ParachainConfig) -> Self {
        Self {
            parachains: vec![self.parachains, vec![f(ParachainConfig::default())]].concat(),
            ..self
        }
    }

    pub fn with_hrmp_channel(self, f: fn(HrmpChannelConfig) -> HrmpChannelConfig) -> Self {
        Self {
            hrmp_channels: vec![self.hrmp_channels, vec![f(HrmpChannelConfig::default())]].concat(),
            ..self
        }
    }
}
