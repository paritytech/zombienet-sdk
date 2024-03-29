use std::{error::Error, fmt::Display, net::IpAddr, str::FromStr};

use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{merge_errors, merge_errors_vecs},
        types::Duration,
    },
    utils::default_node_spawn_timeout,
};

/// Global settings applied to an entire network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalSettings {
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    bootnodes_addresses: Vec<Multiaddr>,
    // TODO: parse both case in zombienet node version to avoid renamed ?
    #[serde(rename = "timeout")]
    network_spawn_timeout: Duration,
    #[serde(default = "default_node_spawn_timeout")]
    node_spawn_timeout: Duration,
    local_ip: Option<IpAddr>,
}

impl GlobalSettings {
    /// External bootnode address.
    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect()
    }

    /// Global spawn timeout in seconds.
    pub fn network_spawn_timeout(&self) -> Duration {
        self.network_spawn_timeout
    }

    /// Individual node spawn timeout in seconds.
    pub fn node_spawn_timeout(&self) -> Duration {
        self.node_spawn_timeout
    }

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    pub fn local_ip(&self) -> Option<&IpAddr> {
        self.local_ip.as_ref()
    }
}

/// A global settings builder, used to build [`GlobalSettings`] declaratively with fields validation.
pub struct GlobalSettingsBuilder {
    config: GlobalSettings,
    errors: Vec<anyhow::Error>,
}

impl Default for GlobalSettingsBuilder {
    fn default() -> Self {
        Self {
            config: GlobalSettings {
                bootnodes_addresses: vec![],
                network_spawn_timeout: 1000,
                node_spawn_timeout: 300,
                local_ip: None,
            },
            errors: vec![],
        }
    }
}

impl GlobalSettingsBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    // Transition to the next state of the builder.
    fn transition(config: GlobalSettings, errors: Vec<anyhow::Error>) -> Self {
        Self { config, errors }
    }

    /// Set the external bootnode address.
    pub fn with_bootnodes_addresses<T>(self, bootnodes_addresses: Vec<T>) -> Self
    where
        T: TryInto<Multiaddr> + Display + Copy,
        T::Error: Error + Send + Sync + 'static,
    {
        let mut addrs = vec![];
        let mut errors = vec![];

        for (index, addr) in bootnodes_addresses.into_iter().enumerate() {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(error) => errors.push(
                    FieldError::BootnodesAddress(index, addr.to_string(), error.into()).into(),
                ),
            }
        }

        Self::transition(
            GlobalSettings {
                bootnodes_addresses: addrs,
                ..self.config
            },
            merge_errors_vecs(self.errors, errors),
        )
    }

    /// Set global spawn timeout in seconds.
    pub fn with_network_spawn_timeout(self, timeout: Duration) -> Self {
        Self::transition(
            GlobalSettings {
                network_spawn_timeout: timeout,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set individual node spawn timeout in seconds.
    pub fn with_node_spawn_timeout(self, timeout: Duration) -> Self {
        Self::transition(
            GlobalSettings {
                node_spawn_timeout: timeout,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set local IP used to expose local services (including RPC, metrics and monitoring).
    pub fn with_local_ip(self, local_ip: &str) -> Self {
        match IpAddr::from_str(local_ip) {
            Ok(local_ip) => Self::transition(
                GlobalSettings {
                    local_ip: Some(local_ip),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::LocalIp(error.into()).into()),
            ),
        }
    }

    /// Seals the builder and returns a [`GlobalSettings`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<GlobalSettings, Vec<anyhow::Error>> {
        if !self.errors.is_empty() {
            return Err(self
                .errors
                .into_iter()
                .map(|error| ConfigError::GlobalSettings(error).into())
                .collect::<Vec<_>>());
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_settings_config_builder_should_succeeds_and_returns_a_global_settings_config() {
        let global_settings_config = GlobalSettingsBuilder::new()
            .with_bootnodes_addresses(vec![
                "/ip4/10.41.122.55/tcp/45421",
                "/ip4/51.144.222.10/tcp/2333",
            ])
            .with_network_spawn_timeout(600)
            .with_node_spawn_timeout(120)
            .with_local_ip("10.0.0.1")
            .build()
            .unwrap();

        let bootnodes_addresses: Vec<Multiaddr> = vec![
            "/ip4/10.41.122.55/tcp/45421".try_into().unwrap(),
            "/ip4/51.144.222.10/tcp/2333".try_into().unwrap(),
        ];
        assert_eq!(
            global_settings_config.bootnodes_addresses(),
            bootnodes_addresses.iter().collect::<Vec<_>>()
        );
        assert_eq!(global_settings_config.network_spawn_timeout(), 600);
        assert_eq!(global_settings_config.node_spawn_timeout(), 120);
        assert_eq!(
            global_settings_config
                .local_ip()
                .unwrap()
                .to_string()
                .as_str(),
            "10.0.0.1"
        );
    }

    #[test]
    fn global_settings_config_builder_should_succeeds_when_node_spawn_timeout_is_missing() {
        let global_settings_config = GlobalSettingsBuilder::new()
            .with_bootnodes_addresses(vec![
                "/ip4/10.41.122.55/tcp/45421",
                "/ip4/51.144.222.10/tcp/2333",
            ])
            .with_network_spawn_timeout(600)
            .with_local_ip("10.0.0.1")
            .build()
            .unwrap();

        let bootnodes_addresses: Vec<Multiaddr> = vec![
            "/ip4/10.41.122.55/tcp/45421".try_into().unwrap(),
            "/ip4/51.144.222.10/tcp/2333".try_into().unwrap(),
        ];
        assert_eq!(
            global_settings_config.bootnodes_addresses(),
            bootnodes_addresses.iter().collect::<Vec<_>>()
        );
        assert_eq!(global_settings_config.network_spawn_timeout(), 600);
        assert_eq!(global_settings_config.node_spawn_timeout(), 300);
        assert_eq!(
            global_settings_config
                .local_ip()
                .unwrap()
                .to_string()
                .as_str(),
            "10.0.0.1"
        );
    }

    #[test]
    fn global_settings_builder_should_fails_and_returns_an_error_if_one_bootnode_address_is_invalid(
    ) {
        let errors = GlobalSettingsBuilder::new()
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421"])
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "global_settings.bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
    }

    #[test]
    fn global_settings_builder_should_fails_and_returns_multiple_errors_if_multiple_bootnodes_addresses_are_invalid(
    ) {
        let errors = GlobalSettingsBuilder::new()
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421", "//10.42.153.10/tcp/43111"])
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "global_settings.bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "global_settings.bootnodes_addresses[1]: '//10.42.153.10/tcp/43111' unknown protocol string: "
        );
    }

    #[test]
    fn global_settings_builder_should_fails_and_returns_an_error_if_local_ip_is_invalid() {
        let errors = GlobalSettingsBuilder::new()
            .with_local_ip("invalid")
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "global_settings.local_ip: invalid IP address syntax"
        );
    }

    #[test]
    fn global_settings_builder_should_fails_and_returns_multiple_errors_if_multiple_fields_are_invalid(
    ) {
        let errors = GlobalSettingsBuilder::new()
            .with_bootnodes_addresses(vec!["/ip4//tcp/45421", "//10.42.153.10/tcp/43111"])
            .with_local_ip("invalid")
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 3);
        assert_eq!(
            errors.first().unwrap().to_string(),
            "global_settings.bootnodes_addresses[0]: '/ip4//tcp/45421' failed to parse: invalid IPv4 address syntax"
        );
        assert_eq!(
            errors.get(1).unwrap().to_string(),
            "global_settings.bootnodes_addresses[1]: '//10.42.153.10/tcp/43111' unknown protocol string: "
        );
        assert_eq!(
            errors.get(2).unwrap().to_string(),
            "global_settings.local_ip: invalid IP address syntax"
        );
    }
}
