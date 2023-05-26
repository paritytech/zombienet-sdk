use core::marker::PhantomData;

use serde::Serialize;

use crate::{
    errors::ConfigError,
    shared::types::{IpAddress, MultiAddress, ParaId, TimeoutInSecs},
    HrmpChannelConfig, ParachainConfig, RelaychainConfig,
};

#[derive(Default, Clone)]
pub struct NoRelayChain;
#[derive(Default, Clone)]
pub struct WithRelayChain;

/// Global settings applied to an entire network.
#[derive(Debug, Serialize)]
pub struct GlobalSettings {
    /// [TODO]: This feature is not well defined, which binary/image should use to spawn the bootnode?
    /// Whether we should spawn a dedicated bootnode for each chain.
    // spawn_bootnode: bool,

    /// External bootnodes addresses.
    /// [TODO]: is it a default overriden by node config, maybe an option ?
    bootnodes_addresses: Vec<MultiAddress>,

    /// Global spawn timeout in seconds.
    network_spawn_timeout: TimeoutInSecs,

    /// Individual node spawn timeout.
    node_spawn_timeout: TimeoutInSecs,

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    local_ip: Option<IpAddress>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            // spawn_bootnode: false,
            bootnode_address:      vec![],
            network_spawn_timeout: 1000,
            node_spawn_timeout:    300,
            local_ip:              None,
        }
    }
}

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
#[derive(Debug, Serialize)]
pub struct NetworkConfig {
    /// The global settings applied to the network.
    global_settings: GlobalSettings,

    /// Relaychain configuration.
    relaychain: RelaychainConfig,

    /// Parachains configurations.
    parachains: Vec<ParachainConfig>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<HrmpChannelConfig>,
}

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
#[derive(Debug, Default)]
pub struct NetworkConfigBuilder<R> {
    /// The global settings applied to the network.
    global_settings: GlobalSettings,

    /// Relaychain configuration.
    relaychain: RelaychainConfig,

    /// Parachains configurations.
    parachains: Vec<ParachainConfig>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<HrmpChannelConfig>,

    typestate: PhantomData<R>,
}

impl NetworkConfigBuilder<NoRelayChain> {
    pub fn new() -> Self {
        NetworkConfigBuilder::default()
    }
}

impl NetworkConfigBuilder<WithRelayChain> {
    pub fn build(self) -> Result<NetworkConfig, ConfigError> {
        // TODO: validate here.
        Ok(NetworkConfig {
            global_settings: self.global_settings,
            relaychain:      self.relaychain,
            parachains:      self.parachains,
            hrmp_channels:   self.hrmp_channels,
        })
    }
}

impl<R> NetworkConfigBuilder<R> {
    pub fn with_global_settings(
        self,
        f: fn(GlobalSettings) -> GlobalSettings,
    ) -> NetworkConfigBuilder<R> {
        NetworkConfigBuilder {
            global_settings: f(self.global_settings),
            ..self
        }
    }
}

impl NetworkConfigBuilder<NoRelayChain> {
    pub fn with_relaychain(
        self,
        f: fn(RelaychainConfig) -> RelaychainConfig,
    ) -> NetworkConfigBuilder<WithRelayChain> {
        NetworkConfigBuilder {
            relaychain:      f(RelaychainConfig::default()),
            global_settings: self.global_settings,
            parachains:      self.parachains,
            hrmp_channels:   self.hrmp_channels,
            typestate:       PhantomData,
        }
    }
}

impl NetworkConfigBuilder<WithRelayChain> {
    pub fn with_parachain(
        self,
        f: fn(ParachainConfig) -> ParachainConfig,
    ) -> NetworkConfigBuilder<WithRelayChain> {
        Self {
            parachains: vec![self.parachains, vec![f(ParachainConfig::default())]].concat(),
            ..self
        }
    }

    pub fn with_hrmp_channel(
        self,
        f: fn(HrmpChannelConfig) -> HrmpChannelConfig,
    ) -> NetworkConfigBuilder<WithRelayChain> {
        Self {
            hrmp_channels: vec![self.hrmp_channels, vec![f(HrmpChannelConfig::default())]].concat(),
            ..self
        }
    }
}

impl NetworkConfig {
    pub(crate) fn global_settings(&self) -> &GlobalSettings {
        &self.global_settings
    }

    pub(crate) fn relaychain(&self) -> &RelaychainConfig {
        &self.relaychain
    }

    pub(crate) fn parachains(&self) -> &Vec<ParachainConfig> {
        &self.parachains
    }

    pub(crate) fn hrmp_channels(&self) -> &Vec<HrmpChannelConfig> {
        &self.hrmp_channels
    }

    //[TODO]: skill serializing empty vec or None
    pub fn dump(&self) -> Result<String, ConfigError> {
        let config_json =
            serde_json::to_string_pretty(&self).map_err(|_| ConfigError::SerializationError)?;
        Ok(config_json)
    }
}
