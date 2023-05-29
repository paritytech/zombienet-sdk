use crate::shared::types::{Duration, IpAddress, MultiAddress};

/// Global settings applied to an entire network.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSettings {
    /// Whether we should spawn a dedicated bootnode for each chain.
    /// TODO: commented now until we decide how we want to use this option
    // spawn_bootnode: bool,

    /// External bootnode address.
    bootnodes_addresses: Vec<MultiAddress>,

    /// Global spawn timeout in seconds.
    network_spawn_timeout: Duration,

    /// Individual node spawn timeout.
    node_spawn_timeout: Duration,

    /// Local IP used to expose local services (including RPC, metrics and monitoring).
    local_ip: Option<IpAddress>,
}

impl GlobalSettings {
    pub fn bootnodes_addresses(&self) -> Vec<&MultiAddress> {
        self.bootnodes_addresses.iter().collect()
    }

    pub fn network_spawn_timeout(&self) -> &Duration {
        &self.network_spawn_timeout
    }

    pub fn node_spawn_timeout(&self) -> &Duration {
        &self.node_spawn_timeout
    }

    pub fn local_ip(&self) -> Option<&IpAddress> {
        self.local_ip.as_ref()
    }
}

#[derive(Debug)]
pub struct GlobalSettingsBuilder {
    config: GlobalSettings,
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
        }
    }
}

impl GlobalSettingsBuilder {
    pub fn new() -> GlobalSettingsBuilder {
        Self::default()
    }

    fn transition(config: GlobalSettings) -> Self {
        Self { config }
    }

    pub fn with_bootnodes_addresses(self, addresses: Vec<MultiAddress>) -> Self {
        Self::transition(GlobalSettings {
            bootnodes_addresses: addresses,
            ..self.config
        })
    }

    pub fn with_network_spawn_timeout(self, timeout: Duration) -> Self {
        Self::transition(GlobalSettings {
            network_spawn_timeout: timeout,
            ..self.config
        })
    }

    pub fn with_spawn_timeout(self, timeout: Duration) -> Self {
        Self::transition(GlobalSettings {
            node_spawn_timeout: timeout,
            ..self.config
        })
    }

    pub fn with_local_ip(self, local_ip: IpAddress) -> Self {
        Self::transition(GlobalSettings {
            local_ip: Some(local_ip),
            ..self.config
        })
    }

    pub fn build(self) -> GlobalSettings {
        self.config
    }
}
