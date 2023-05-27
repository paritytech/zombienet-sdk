use std::marker::PhantomData;

use crate::{
    hrmp_channel::{self, HrmpChannelConfigBuilder},
    parachain::{self, ParachainConfigBuilder},
    relaychain::{self, RelaychainConfigBuilder},
    shared::{
        macros::states,
        types::{Duration, IpAddress, MultiAddress},
    },
    HrmpChannelConfig, ParachainConfig, RelaychainConfig,
};

/// Global settings applied to an entire network.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSettings {
    /// Whether we should spawn a dedicated bootnode for each chain.
    /// TODO: commented now until we decide how we want to use this option
    // spawn_bootnode: bool,

    /// External bootnode address.
    /// TODO: is it a default overriden by node config, maybe an option ?
    bootnodes_addresses: Vec<MultiAddress>,

    /// Global spawn timeout in seconds.
    network_spawn_timeout: Duration,

    /// Individual node spawn timeout.
    node_spawn_timeout: Duration,

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    local_ip: Option<IpAddress>,
}

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfig {
    /// The global settings applied to the network.
    // global_settings: GlobalSettings,

    // /// Relaychain configuration.
    // relaychain: RelaychainConfig,

    /// Parachains configurations.
    parachains: Vec<ParachainConfig>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<HrmpChannelConfig>,
}

impl NetworkConfig {
    // pub fn global_settings(&self) -> &GlobalSettings {
    //     &self.global_settings
    // }

    // pub fn relaychain(&self) -> &RelaychainConfig {
    //     &self.relaychain
    // }

    pub fn parachains(&self) -> Vec<&ParachainConfig> {
        self.parachains.iter().collect::<Vec<_>>()
    }

    pub fn hrmp_channels(&self) -> Vec<&HrmpChannelConfig> {
        self.hrmp_channels.iter().collect::<Vec<_>>()
    }
}

states! {
    Initial,
    WithRelaychain
}

#[derive(Debug)]
pub struct NetworkConfigBuilder<State> {
    config: NetworkConfig,
    _state: PhantomData<State>,
}

impl<A> NetworkConfigBuilder<A> {
    fn transition<B>(config: NetworkConfig) -> NetworkConfigBuilder<B> {
        NetworkConfigBuilder {
            config,
            _state: PhantomData,
        }
    }
}

impl NetworkConfigBuilder<Initial> {
    pub fn new() -> NetworkConfigBuilder<Initial> {
        NetworkConfigBuilder {
            config: NetworkConfig {
                // global_settings: GlobalSettings {
                //     bootnodes_addresses: vec![],
                //     network_spawn_timeout: 1000,
                //     node_spawn_timeout: 300,
                //     local_ip: None,
                // },
                // relaychain: RelaychainConfigBuilder::new()
                //     .with_chain("")
                //     .with_node(|node| node.with_name("").with_command(""))
                //     .build(),
                parachains: vec![],
                hrmp_channels: vec![],
            },
            _state: PhantomData,
        }
    }
}

impl NetworkConfigBuilder<WithRelaychain> {
    pub fn with_parachain(
        self,
        f: fn(
            ParachainConfigBuilder<parachain::Initial>,
        ) -> ParachainConfigBuilder<parachain::WithAtLeastOneCollator>,
    ) -> Self {
        let new_parachain = f(ParachainConfigBuilder::new()).build();

        Self::transition(NetworkConfig {
            parachains: vec![self.config.parachains, vec![new_parachain]].concat(),
            ..self.config
        })
    }

    pub fn with_hrmp_channel(
        self,
        f: fn(
            HrmpChannelConfigBuilder<hrmp_channel::Initial>,
        ) -> HrmpChannelConfigBuilder<hrmp_channel::WithRecipient>,
    ) -> Self {
        let new_hrmp_channel = f(HrmpChannelConfigBuilder::new()).build();

        Self::transition(NetworkConfig {
            hrmp_channels: vec![self.config.hrmp_channels, vec![new_hrmp_channel]].concat(),
            ..self.config
        })
    }
}
