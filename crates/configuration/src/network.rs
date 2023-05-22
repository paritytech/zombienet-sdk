use crate::{
    parachain::ParachainConfig,
    relaychain::RelaychainConfig,
    shared::types::{IpAddress, MultiAddress, Timeout},
};

/// Global settings applied to an entire network.
#[derive(Debug, Clone)]
struct GlobalSettings {
    /// Whether we should spawn a dedicated bootnode for each chain.
    spawn_bootnode: bool,

    /// External bootnode address.
    /// [TODO]: is it a default overriden by node config, maybe an option ?
    bootnode_address: MultiAddress,

    /// Global spawn timeout in seconds.
    spawn_timeout: Timeout,

    /// Individual node spawn timeout.
    node_spawn_timeout: Timeout,

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    local_ip: Option<IpAddress>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            ..Default::default()
        }
    }
}

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// The global settings applied to the network.
    global_settings: GlobalSettings,

    /// Relaychain configuration.
    relaychain: RelaychainConfig,

    /// Parachains configurations.
    parachains: Vec<ParachainConfig>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<()>,

    // [TODO]: what does it represents ?
    config_base_path: String,

    // [TODO]: what does it represents ?
    seed: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        // [TODO]: define the default value for a network
        todo!()
    }
}
