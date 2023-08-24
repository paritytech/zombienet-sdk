use std::{cell::RefCell, marker::PhantomData, rc::Rc};

use regex::Regex;
use serde::Serialize;

use crate::{
    global_settings::{GlobalSettings, GlobalSettingsBuilder},
    hrmp_channel::{self, HrmpChannelConfig, HrmpChannelConfigBuilder},
    parachain::{self, ParachainConfig, ParachainConfigBuilder},
    relaychain::{self, RelaychainConfig, RelaychainConfigBuilder},
    shared::{helpers::merge_errors_vecs, macros::states, types::ValidationContext},
};

/// A network configuration, composed of a relaychain, parachains and HRMP channels.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkConfig {
    #[serde(rename = "settings")]
    global_settings: GlobalSettings,
    relaychain: Option<RelaychainConfig>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty")]
    parachains: Vec<ParachainConfig>,
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty")]
    hrmp_channels: Vec<HrmpChannelConfig>,
}

impl NetworkConfig {
    /// The global settings of the network.
    pub fn global_settings(&self) -> &GlobalSettings {
        &self.global_settings
    }

    /// The relay chain of the network.
    pub fn relaychain(&self) -> &RelaychainConfig {
        self.relaychain
            .as_ref()
            .expect("typestate should ensure the relaychain isn't None at this point, this is a bug please report it: https://github.com/paritytech/zombienet-sdk/issues")
    }

    /// The parachains of the network.
    pub fn parachains(&self) -> Vec<&ParachainConfig> {
        self.parachains.iter().collect::<Vec<_>>()
    }

    /// The HRMP channels of the network.
    pub fn hrmp_channels(&self) -> Vec<&HrmpChannelConfig> {
        self.hrmp_channels.iter().collect::<Vec<_>>()
    }

    pub fn dump_to_toml(&self) -> Result<String, toml::ser::Error> {
        // This regex is used to replace the "" enclosed u128 value to a raw u128 because u128 is not supported for TOML serialization/deserialization.
        let re = Regex::new(r#""U128%(?<u128_value>\d+)""#).expect("regex should be valid, this is a bug please report it: https://github.com/paritytech/zombienet-sdk/issues");
        let toml_string = toml::to_string_pretty(&self)?;

        Ok(re.replace_all(&toml_string, "$u128_value").to_string())
    }
}

states! {
    Initial,
    WithRelaychain
}

/// A network configuration builder, used to build a [`NetworkConfig`] declaratively with fields validation.
///
/// # Example:
///
/// ```
/// use configuration::NetworkConfigBuilder;
///
/// let network_config = NetworkConfigBuilder::new()
///     .with_relaychain(|relaychain| {
///         relaychain
///             .with_chain("polkadot")
///             .with_random_nominators_count(10)
///             .with_default_resources(|resources| {
///                 resources
///                     .with_limit_cpu("1000m")
///                     .with_request_memory("1Gi")
///                     .with_request_cpu(100_000)
///             })
///             .with_node(|node| {
///                 node.with_name("node")
///                     .with_command("command")
///                     .validator(true)
///             })
///     })
///     .with_parachain(|parachain| {
///         parachain
///             .with_id(1000)
///             .with_chain("myparachain1")
///             .with_initial_balance(100_000)
///             .with_default_image("myimage:version")
///             .with_collator(|collator| {
///                 collator
///                     .with_name("collator1")
///                     .with_command("command1")
///                     .validator(true)
///             })
///     })
///     .with_parachain(|parachain| {
///         parachain
///             .with_id(2000)
///             .with_chain("myparachain2")
///             .with_initial_balance(50_0000)
///             .with_collator(|collator| {
///                 collator
///                     .with_name("collator2")
///                     .with_command("command2")
///                     .validator(true)
///             })
///     })
///     .with_hrmp_channel(|hrmp_channel1| {
///         hrmp_channel1
///             .with_sender(1)
///             .with_recipient(2)
///             .with_max_capacity(200)
///             .with_max_message_size(500)
///     })
///     .with_hrmp_channel(|hrmp_channel2| {
///         hrmp_channel2
///             .with_sender(2)
///             .with_recipient(1)
///             .with_max_capacity(100)
///             .with_max_message_size(250)
///     })
///     .with_global_settings(|global_settings| {
///         global_settings
///             .with_network_spawn_timeout(1200)
///             .with_node_spawn_timeout(240)
///     })
///     .build();
///
/// assert!(network_config.is_ok())
/// ```
#[derive(Debug)]
pub struct NetworkConfigBuilder<State> {
    config: NetworkConfig,
    validation_context: Rc<RefCell<ValidationContext>>,
    errors: Vec<anyhow::Error>,
    _state: PhantomData<State>,
}

impl Default for NetworkConfigBuilder<Initial> {
    fn default() -> Self {
        Self {
            config: NetworkConfig {
                global_settings: GlobalSettingsBuilder::new().build().expect(
                    "should have no errors for default builder. this is a bug, please report it",
                ),
                relaychain: None,
                parachains: vec![],
                hrmp_channels: vec![],
            },
            validation_context: Default::default(),
            errors: vec![],
            _state: PhantomData,
        }
    }
}

impl<A> NetworkConfigBuilder<A> {
    fn transition<B>(
        config: NetworkConfig,
        validation_context: Rc<RefCell<ValidationContext>>,
        errors: Vec<anyhow::Error>,
    ) -> NetworkConfigBuilder<B> {
        NetworkConfigBuilder {
            config,
            errors,
            validation_context,
            _state: PhantomData,
        }
    }
}

impl NetworkConfigBuilder<Initial> {
    pub fn new() -> NetworkConfigBuilder<Initial> {
        Self::default()
    }

    /// Set the relay chain using a nested [`RelaychainConfigBuilder`].
    pub fn with_relaychain(
        self,
        f: fn(
            RelaychainConfigBuilder<relaychain::Initial>,
        ) -> RelaychainConfigBuilder<relaychain::WithAtLeastOneNode>,
    ) -> NetworkConfigBuilder<WithRelaychain> {
        match f(RelaychainConfigBuilder::new(
            self.validation_context.clone(),
        ))
        .build()
        {
            Ok(relaychain) => Self::transition(
                NetworkConfig {
                    relaychain: Some(relaychain),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(errors) => Self::transition(self.config, self.validation_context, errors),
        }
    }
}

impl NetworkConfigBuilder<WithRelaychain> {
    /// Set the global settings using a nested [`GlobalSettingsBuilder`].
    pub fn with_global_settings(
        self,
        f: fn(GlobalSettingsBuilder) -> GlobalSettingsBuilder,
    ) -> Self {
        match f(GlobalSettingsBuilder::new()).build() {
            Ok(global_settings) => Self::transition(
                NetworkConfig {
                    global_settings,
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(self.errors, errors),
            ),
        }
    }

    /// Add a parachain using a nested [`ParachainConfigBuilder`].
    pub fn with_parachain(
        self,
        f: fn(
            ParachainConfigBuilder<parachain::Initial>,
        ) -> ParachainConfigBuilder<parachain::WithAtLeastOneCollator>,
    ) -> Self {
        match f(ParachainConfigBuilder::new(self.validation_context.clone())).build() {
            Ok(parachain) => Self::transition(
                NetworkConfig {
                    parachains: [self.config.parachains, vec![parachain]].concat(),
                    ..self.config
                },
                self.validation_context,
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
                self.validation_context,
                merge_errors_vecs(self.errors, errors),
            ),
        }
    }

    /// Add an HRMP channel using a nested [`HrmpChannelConfigBuilder`].
    pub fn with_hrmp_channel(
        self,
        f: fn(
            HrmpChannelConfigBuilder<hrmp_channel::Initial>,
        ) -> HrmpChannelConfigBuilder<hrmp_channel::WithRecipient>,
    ) -> Self {
        let new_hrmp_channel = f(HrmpChannelConfigBuilder::new()).build();

        Self::transition(
            NetworkConfig {
                hrmp_channels: [self.config.hrmp_channels, vec![new_hrmp_channel]].concat(),
                ..self.config
            },
            self.validation_context,
            self.errors,
        )
    }

    /// Seals the builder and returns a [`NetworkConfig`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<NetworkConfig, Vec<anyhow::Error>> {
        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::shared::types::RegistrationStrategy;

    #[test]
    fn network_config_builder_should_succeeds_and_returns_a_network_config() {
        let network_config = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1)
                    .with_chain("myparachain1")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator1")
                            .with_command("command1")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(2)
                    .with_chain("myparachain2")
                    .with_initial_balance(0)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator2")
                            .with_command("command2")
                            .validator(true)
                    })
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
            .build()
            .unwrap();

        // relaychain
        assert_eq!(network_config.relaychain().chain().as_str(), "polkadot");
        assert_eq!(network_config.relaychain().nodes().len(), 1);
        let &node = network_config.relaychain().nodes().first().unwrap();
        assert_eq!(node.name(), "node");
        assert_eq!(node.command().unwrap().as_str(), "command");
        assert!(node.is_validator());
        assert_eq!(
            network_config
                .relaychain()
                .random_nominators_count()
                .unwrap(),
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
        assert_eq!(collator.command().unwrap().as_str(), "command1");
        assert!(collator.is_validator());
        assert_eq!(parachain1.initial_balance(), 100_000);

        // parachain2
        let &parachain2 = network_config.parachains().last().unwrap();
        assert_eq!(parachain2.id(), 2);
        assert_eq!(parachain2.collators().len(), 1);
        let &collator = parachain2.collators().first().unwrap();
        assert_eq!(collator.name(), "collator2");
        assert_eq!(collator.command().unwrap().as_str(), "command2");
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

    #[test]
    fn network_config_builder_should_fails_and_returns_multiple_errors_if_relaychain_is_invalid() {
        let errors = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_default_image("invalid.image")
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("invalid command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1)
                    .with_chain("myparachain")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator1")
                            .with_command("command1")
                            .validator(true)
                    })
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "relaychain.default_image: 'invalid.image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "relaychain.nodes['node'].command: 'invalid command' shouldn't contains whitespace"
        );
    }

    #[test]
    fn network_config_builder_should_fails_and_returns_multiple_errors_if_parachain_is_invalid() {
        let errors = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1000)
                    .with_chain("myparachain")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator1")
                            .with_command("invalid command")
                            .with_image("invalid.image")
                            .validator(true)
                    })
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[1000].collators['collator1'].command: 'invalid command' shouldn't contains whitespace"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "parachain[1000].collators['collator1'].image: 'invalid.image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn network_config_builder_should_fails_and_returns_multiple_errors_if_multiple_parachains_are_invalid(
    ) {
        let errors = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1000)
                    .with_chain("myparachain1")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator1")
                            .with_command("invalid command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(2000)
                    .with_chain("myparachain2")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator2")
                            .validator(true)
                            .with_resources(|resources| {
                                resources
                                    .with_limit_cpu("1000m")
                                    .with_request_memory("1Gi")
                                    .with_request_cpu("invalid")
                            })
                    })
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "parachain[1000].collators['collator1'].command: 'invalid command' shouldn't contains whitespace"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "parachain[2000].collators['collator2'].resources.request_cpu: 'invalid' doesn't match regex '^\\d+(.\\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$'"
        );
    }

    #[test]
    fn network_config_builder_should_fails_and_returns_multiple_errors_if_global_settings_is_invalid(
    ) {
        let errors = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1000)
                    .with_chain("myparachain")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator")
                            .with_command("command")
                            .validator(true)
                    })
            })
            .with_global_settings(|global_settings| {
                global_settings
                    .with_local_ip("127.0.0000.1")
                    .with_bootnodes_addresses(vec!["/ip4//tcp/45421"])
            })
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "global_settings.local_ip: invalid IP address syntax"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "global_settings.bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
    }

    #[test]
    fn network_config_builder_should_fails_and_returns_multiple_errors_if_multiple_fields_are_invalid(
    ) {
        let errors = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_random_nominators_count(10)
                    .with_node(|node| {
                        node.with_name("node")
                            .with_command("invalid command")
                            .validator(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1000)
                    .with_chain("myparachain")
                    .with_initial_balance(100_000)
                    .with_collator(|collator| {
                        collator
                            .with_name("collator")
                            .with_command("command")
                            .with_image("invalid.image")
                            .validator(true)
                    })
            })
            .with_global_settings(|global_settings| global_settings.with_local_ip("127.0.0000.1"))
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 3);
        assert_eq!(
            errors.get(0).unwrap().to_string(),
            "relaychain.nodes['node'].command: 'invalid command' shouldn't contains whitespace"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "parachain[1000].collators['collator'].image: 'invalid.image' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
        assert_eq!(
            errors.get(2).unwrap().to_string(),
            "global_settings.local_ip: invalid IP address syntax"
        );
    }

    #[test]
    fn network_config_should_be_dumpable_to_a_toml_config_for_a_small_network() {
        let network_config = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("rococo-local")
                    .with_default_command("polkadot")
                    .with_default_image("docker.io/parity/polkadot:latest")
                    .with_default_args(vec![("-lparachain", "debug").into()])
                    .with_node(|node| {
                        node.with_name("alice")
                            .validator(true)
                            .invulnerable(true)
                            .validator(true)
                    })
                    .with_node(|node| {
                        node.with_name("bob")
                            .validator(true)
                            .bootnode(true)
                            .with_args(vec![("--database", "paritydb-experimental").into()])
                    })
            })
            .build()
            .unwrap();

        let got = network_config.dump_to_toml().unwrap();
        let expected = fs::read_to_string("./testing/snapshots/0000-small-network.toml").unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn network_config_should_be_dumpable_to_a_toml_config_for_a_big_network() {
        let network_config = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_default_command("polkadot")
                    .with_default_image("docker.io/parity/polkadot:latest")
                    .with_default_resources(|resources| {
                        resources
                            .with_request_cpu(100000)
                            .with_request_memory("500M")
                            .with_limit_cpu("10Gi")
                            .with_limit_memory("4000M")
                    })
                    .with_node(|node| {
                        node.with_name("alice")
                            .with_initial_balance(1_000_000_000)
                            .validator(true)
                            .bootnode(true)
                            .invulnerable(true)
                    })
                    .with_node(|node| {
                        node.with_name("bob")
                            .validator(true)
                            .invulnerable(true)
                            .bootnode(true)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1000)
                    .with_chain("myparachain")
                    .with_chain_spec_path("/path/to/my/chain/spec.json")
                    .with_registration_strategy(RegistrationStrategy::UsingExtrinsic)
                    .onboard_as_parachain(false)
                    .with_default_db_snapshot("https://storage.com/path/to/db_snapshot.tgz")
                    .with_collator(|collator| {
                        collator
                            .with_name("john")
                            .bootnode(true)
                            .validator(true)
                            .invulnerable(true)
                            .with_initial_balance(5_000_000_000)
                    })
                    .with_collator(|collator| {
                        collator
                            .with_name("charles")
                            .bootnode(true)
                            .invulnerable(true)
                            .with_initial_balance(0)
                    })
                    .with_collator(|collator| {
                        collator
                            .with_name("frank")
                            .validator(true)
                            .bootnode(true)
                            .with_initial_balance(1_000_000_000)
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(2000)
                    .with_chain("myotherparachain")
                    .with_chain_spec_path("/path/to/my/other/chain/spec.json")
                    .with_collator(|collator| {
                        collator
                            .with_name("mike")
                            .bootnode(true)
                            .validator(true)
                            .invulnerable(true)
                            .with_initial_balance(5_000_000_000)
                    })
                    .with_collator(|collator| {
                        collator
                            .with_name("georges")
                            .bootnode(true)
                            .invulnerable(true)
                            .with_initial_balance(0)
                    })
                    .with_collator(|collator| {
                        collator
                            .with_name("victor")
                            .validator(true)
                            .bootnode(true)
                            .with_initial_balance(1_000_000_000)
                    })
            })
            .with_hrmp_channel(|hrmp_channel| {
                hrmp_channel
                    .with_sender(1000)
                    .with_recipient(2000)
                    .with_max_capacity(150)
                    .with_max_message_size(5000)
            })
            .with_hrmp_channel(|hrmp_channel| {
                hrmp_channel
                    .with_sender(2000)
                    .with_recipient(1000)
                    .with_max_capacity(200)
                    .with_max_message_size(8000)
            })
            .build()
            .unwrap();

        let got = network_config.dump_to_toml().unwrap();
        let expected = fs::read_to_string("./testing/snapshots/0001-big-network.toml").unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn network_config_builder_should_be_dumplable_to_a_toml_config_a_overrides_default_correctly() {
        let network_config = NetworkConfigBuilder::new()
            .with_relaychain(|relaychain| {
                relaychain
                    .with_chain("polkadot")
                    .with_default_command("polkadot")
                    .with_default_image("docker.io/parity/polkadot:latest")
                    .with_default_args(vec![("-name", "value").into(), "--flag".into()])
                    .with_default_db_snapshot("https://storage.com/path/to/db_snapshot.tgz")
                    .with_default_resources(|resources| {
                        resources
                            .with_request_cpu(100000)
                            .with_request_memory("500M")
                            .with_limit_cpu("10Gi")
                            .with_limit_memory("4000M")
                    })
                    .with_node(|node| {
                        node.with_name("alice")
                            .with_initial_balance(1_000_000_000)
                            .validator(true)
                            .bootnode(true)
                            .invulnerable(true)
                    })
                    .with_node(|node| {
                        node.with_name("bob")
                            .validator(true)
                            .invulnerable(true)
                            .bootnode(true)
                            .with_image("mycustomimage:latest")
                            .with_command("my-custom-command")
                            .with_db_snapshot("https://storage.com/path/to/other/db_snapshot.tgz")
                            .with_resources(|resources| {
                                resources
                                    .with_request_cpu(1000)
                                    .with_request_memory("250Mi")
                                    .with_limit_cpu("5Gi")
                                    .with_limit_memory("2Gi")
                            })
                            .with_args(vec![("-myothername", "value").into()])
                    })
            })
            .with_parachain(|parachain| {
                parachain
                    .with_id(1000)
                    .with_chain("myparachain")
                    .with_chain_spec_path("/path/to/my/chain/spec.json")
                    .with_default_db_snapshot("https://storage.com/path/to/other_snapshot.tgz")
                    .with_default_command("my-default-command")
                    .with_default_image("mydefaultimage:latest")
                    .with_collator(|collator| {
                        collator
                            .with_name("john")
                            .bootnode(true)
                            .validator(true)
                            .invulnerable(true)
                            .with_initial_balance(5_000_000_000)
                            .with_command("my-non-default-command")
                            .with_image("anotherimage:latest")
                    })
                    .with_collator(|collator| {
                        collator
                            .with_name("charles")
                            .bootnode(true)
                            .invulnerable(true)
                            .with_initial_balance(0)
                    })
            })
            .build()
            .unwrap();

        let got = network_config.dump_to_toml().unwrap();
        let expected =
            fs::read_to_string("./testing/snapshots/0002-overridden-defaults.toml").unwrap();
        assert_eq!(got, expected);
    }
}
