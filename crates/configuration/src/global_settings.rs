use std::{
    error::Error,
    fmt::Display,
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use multiaddr::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{
    shared::{
        errors::{ConfigError, FieldError},
        helpers::{merge_errors, merge_errors_vecs},
        types::Duration,
    },
    utils::{default_node_spawn_timeout, default_timeout},
};

/// Global settings applied to an entire network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalSettings {
    /// Global bootnodes to use (we will then add more)
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    bootnodes_addresses: Vec<Multiaddr>,
    // TODO: parse both case in zombienet node version to avoid renamed ?
    /// Global spawn timeout
    #[serde(rename = "timeout", default = "default_timeout")]
    network_spawn_timeout: Duration,
    // TODO: not used yet
    /// Node spawn timeout
    #[serde(default = "default_node_spawn_timeout")]
    node_spawn_timeout: Duration,
    // TODO: not used yet
    /// Local ip to use for construct the direct links
    local_ip: Option<IpAddr>,
    /// Directory to use as base dir
    /// Used to reuse the same files (database) from a previous run,
    /// also note that we will override the content of some of those files.
    base_dir: Option<PathBuf>,
    /// Number of concurrent spawning process to launch, None means try to spawn all at the same time.
    spawn_concurrency: Option<usize>,
    /// If enabled, will launch a task to monitor nodes' liveness and tear down the network if there are any.
    #[serde(default)]
    tear_down_on_failure: bool,
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

    /// Base directory to use (instead a random tmp one)
    /// All the artifacts will be created in this directory.
    pub fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }

    /// Number of concurrent spawning process to launch
    pub fn spawn_concurrency(&self) -> Option<usize> {
        self.spawn_concurrency
    }

    /// A flag to tear down the network if there are any unresponsive nodes detected.
    pub fn tear_down_on_failure(&self) -> bool {
        self.tear_down_on_failure
    }
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            bootnodes_addresses: Default::default(),
            network_spawn_timeout: default_timeout(),
            node_spawn_timeout: default_node_spawn_timeout(),
            local_ip: Default::default(),
            base_dir: Default::default(),
            spawn_concurrency: Default::default(),
            tear_down_on_failure: Default::default(),
        }
    }
}

/// A global settings builder, used to build [`GlobalSettings`] declaratively with fields validation.
#[derive(Default)]
pub struct GlobalSettingsBuilder {
    config: GlobalSettings,
    errors: Vec<anyhow::Error>,
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

    /// Set the directory to use as base (instead of a random tmp one).
    pub fn with_base_dir(self, base_dir: impl Into<PathBuf>) -> Self {
        Self::transition(
            GlobalSettings {
                base_dir: Some(base_dir.into()),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the spawn concurrency
    pub fn with_spawn_concurrency(self, spawn_concurrency: usize) -> Self {
        Self::transition(
            GlobalSettings {
                spawn_concurrency: Some(spawn_concurrency),
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the `tear_down_on_failure` flag
    pub fn with_tear_down_on_failure(self, tear_down_on_failure: bool) -> Self {
        Self::transition(
            GlobalSettings {
                tear_down_on_failure,
                ..self.config
            },
            self.errors,
        )
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
            .with_base_dir("/home/nonroot/mynetwork")
            .with_spawn_concurrency(5)
            .with_tear_down_on_failure(true)
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
        assert_eq!(
            global_settings_config.base_dir().unwrap(),
            Path::new("/home/nonroot/mynetwork")
        );
        assert_eq!(global_settings_config.spawn_concurrency().unwrap(), 5);
        assert!(global_settings_config.tear_down_on_failure());
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
        assert_eq!(global_settings_config.node_spawn_timeout(), 600);
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
