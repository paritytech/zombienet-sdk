use std::marker::PhantomData;

use crate::{
    global_settings::{GlobalSettings, GlobalSettingsBuilder},
    hrmp_channel::{self, HrmpChannelConfig, HrmpChannelConfigBuilder},
    parachain::{self, ParachainConfig, ParachainConfigBuilder},
    relaychain::{self, RelaychainConfig, RelaychainConfigBuilder},
    shared::macros::states,
};

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkConfig {
    // The global settings applied to the network.
    global_settings: GlobalSettings,

    /// Relaychain configuration.
    relaychain: Option<RelaychainConfig>,

    /// Parachains configurations.
    parachains: Vec<ParachainConfig>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<HrmpChannelConfig>,
}

impl NetworkConfig {
    pub fn global_settings(&self) -> &GlobalSettings {
        &self.global_settings
    }

    pub fn relaychain(&self) -> &RelaychainConfig {
        self.relaychain
            .as_ref()
            .expect("typestate should ensure the relaychain isn't None at this point, this is a bug please report it")
    }

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

impl Default for NetworkConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: NetworkConfig {
                global_settings: GlobalSettingsBuilder::new().build(),
                relaychain:      None,
                parachains:      vec![],
                hrmp_channels:   vec![],
            },
            _state: PhantomData,
        }
    }
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
        Self::default()
    }

    pub fn with_relaychain(
        self,
        f: fn(
            RelaychainConfigBuilder<relaychain::Initial>,
        ) -> RelaychainConfigBuilder<relaychain::WithAtLeastOneNode>,
    ) -> NetworkConfigBuilder<WithRelaychain> {
        Self::transition(NetworkConfig {
            relaychain: Some(f(RelaychainConfigBuilder::new()).build()),
            ..self.config
        })
    }
}

impl NetworkConfigBuilder<WithRelaychain> {
    pub fn with_global_settings(
        self,
        f: fn(GlobalSettingsBuilder) -> GlobalSettingsBuilder,
    ) -> Self {
        Self::transition(NetworkConfig {
            global_settings: f(GlobalSettingsBuilder::new()).build(),
            ..self.config
        })
    }

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

    pub fn build(self) -> NetworkConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_config_builder_should_build_a_new_network_config_correctly() {
        let network_config = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("node command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain1| {
                parachain1
                    .with_id(1)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator1")
                            .with_command("collator1 command")
                            .validator(true)
                    })
                    .with_initial_balance(100_000)
            })
            .with_parachain(|parachain2| {
                parachain2
                    .with_id(2)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator2")
                            .with_command("collator2 command")
                            .validator(true)
                    })
                    .with_initial_balance(0)
            })
            .with_hrmp_channel(|hrmp_channel1| {
                hrmp_channel1
                    .with_sender(1)
                    .with_recipient(2)
                    .with_max_capacity(200)
                    .with_max_message_size(500)
            })
            .with_hrmp_channel(|hrmp_channel2| {
                hrmp_channel2
                    .with_sender(2)
                    .with_recipient(1)
                    .with_max_capacity(100)
                    .with_max_message_size(250)
            })
            .with_global_settings(|global_settings| {
                global_settings
                    .with_network_spawn_timeout(1200)
                    .with_node_spawn_timeout(240)
            })
            .build();

        // relaychain
        assert_eq!(network_config.relaychain().chain(), "polkadot");
        assert_eq!(network_config.relaychain().nodes().len(), 1);
        let &node = network_config.relaychain().nodes().first().unwrap();
        assert_eq!(node.name(), "node");
        assert_eq!(node.command().unwrap(), "node command");
        assert!(node.is_validator());
        assert_eq!(
            network_config.relaychain().random_minators_count().unwrap(),
            10
        );

        // parachains
        assert_eq!(network_config.parachains().len(), 2);

        // parachain1
        let &parachain1 = network_config.parachains().first().unwrap();
        assert_eq!(parachain1.id(), 1);
        assert_eq!(parachain1.collators().len(), 1);
        let &collator = parachain1.collators().first().unwrap();
        assert_eq!(collator.name(), "collator1");
        assert_eq!(collator.command().unwrap(), "collator1 command");
        assert!(collator.is_validator());
        assert_eq!(parachain1.initial_balance(), 100_000);

        // parachain2
        let &parachain2 = network_config.parachains().last().unwrap();
        assert_eq!(parachain2.id(), 2);
        assert_eq!(parachain2.collators().len(), 1);
        let &collator = parachain2.collators().first().unwrap();
        assert_eq!(collator.name(), "collator2");
        assert_eq!(collator.command().unwrap(), "collator2 command");
        assert!(collator.is_validator());
        assert_eq!(parachain2.initial_balance(), 0);

        // hrmp_channels
        assert_eq!(network_config.hrmp_channels().len(), 2);

        // hrmp_channel1
        let &hrmp_channel1 = network_config.hrmp_channels().first().unwrap();
        assert_eq!(hrmp_channel1.sender(), 1);
        assert_eq!(hrmp_channel1.recipient(), 2);
        assert_eq!(hrmp_channel1.max_capacity(), 200);
        assert_eq!(hrmp_channel1.max_message_size(), 500);

        // hrmp_channel2
        let &hrmp_channel2 = network_config.hrmp_channels().last().unwrap();
        assert_eq!(hrmp_channel2.sender(), 2);
        assert_eq!(hrmp_channel2.recipient(), 1);
        assert_eq!(hrmp_channel2.max_capacity(), 100);
        assert_eq!(hrmp_channel2.max_message_size(), 250);

        // global settings
        assert_eq!(
            network_config.global_settings().network_spawn_timeout(),
            1200
        );
        assert_eq!(network_config.global_settings().node_spawn_timeout(), 240);
    }
}
