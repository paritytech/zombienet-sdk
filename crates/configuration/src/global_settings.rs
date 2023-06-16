use std::{error::Error, net::IpAddr, str::FromStr};

use multiaddr::Multiaddr;

use crate::shared::{
    errors::FieldError,
    helpers::{merge_errors, merge_errors_vecs},
    types::Duration,
};

/// Global settings applied to an entire network.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSettings {
    /// Whether we should spawn a dedicated bootnode for each chain.
    /// TODO: commented now until we decide how we want to use this option
    // spawn_bootnode: bool,

    /// External bootnode address.
    bootnodes_addresses: Vec<Multiaddr>,

    /// Global spawn timeout in seconds.
    network_spawn_timeout: Duration,

    /// Individual node spawn timeout.
    node_spawn_timeout: Duration,

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    local_ip: Option<IpAddr>,
}

impl GlobalSettings {
    pub fn bootnodes_addresses(&self) -> Vec<&Multiaddr> {
        self.bootnodes_addresses.iter().collect()
    }

    pub fn network_spawn_timeout(&self) -> Duration {
        self.network_spawn_timeout
    }

    pub fn node_spawn_timeout(&self) -> Duration {
        self.node_spawn_timeout
    }

    pub fn local_ip(&self) -> Option<&IpAddr> {
        self.local_ip.as_ref()
    }
}

#[derive(Debug)]
pub struct GlobalSettingsBuilder {
    config: GlobalSettings,
    errors: Vec<Box<dyn Error>>,
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

    fn transition(config: GlobalSettings, errors: Vec<Box<dyn Error>>) -> Self {
        Self { config, errors }
    }

    pub fn with_bootnodes_addresses<T>(self, bootnode_addresses: Vec<T>) -> Self
    where
        T: TryInto<Multiaddr>,
        T::Error: Error + 'static,
    {
        let mut addrs = vec![];
        let mut errors = vec![];

        for addr in bootnode_addresses {
            match addr.try_into() {
                Ok(addr) => addrs.push(addr),
                Err(error) => errors.push(error.into()),
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

    pub fn with_network_spawn_timeout(self, timeout: Duration) -> Self {
        Self::transition(
            GlobalSettings {
                network_spawn_timeout: timeout,
                ..self.config
            },
            self.errors,
        )
    }

    pub fn with_node_spawn_timeout(self, timeout: Duration) -> Self {
        Self::transition(
            GlobalSettings {
                node_spawn_timeout: timeout,
                ..self.config
            },
            self.errors,
        )
    }

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
                merge_errors(self.errors, FieldError::InvalidLocalIp(error).into()),
            ),
        }
    }

    pub fn build(self) -> Result<GlobalSettings, Vec<Box<dyn Error>>> {
        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_settings_config_builder_should_build_a_new_global_settings_config_correctly() {
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
}
