use std::collections::HashMap;

use configuration::{shared::types::Port, HrmpChannelConfig, NetworkConfig, GlobalSettings};

use crate::errors::OrchestratorError;

mod node;
mod parachain;
mod relaychain;

use self::{parachain::ParachainSpec, relaychain::RelaychainSpec};

pub struct NetworkSpec {
    /// Relaychain configuration.
    relaychain: RelaychainSpec,

    /// Parachains configurations.
    parachains: Vec<ParachainSpec>,

    /// HRMP channels configurations.
    hrmp_channels: Vec<HrmpChannelConfig>,

    /// Global settings
    global_settings: GlobalSettings,
}

impl NetworkSpec {
    pub async fn from_config(
        network_config: &NetworkConfig,
    ) -> Result<NetworkSpec, OrchestratorError> {
        let mut errs = vec![];
        let relaychain = RelaychainSpec::from_config(network_config.relaychain())?;
        let mut parachains = vec![];

        for para_config in network_config.parachains() {
            match ParachainSpec::from_config(para_config) {
                Ok(para) => parachains.push(para),
                Err(err) => errs.push(err),
            }
        }

        Ok(NetworkSpec {
            relaychain,
            parachains,
            hrmp_channels: network_config
                .hrmp_channels()
                .into_iter()
                .cloned()
                .collect(),
            global_settings: network_config.global_settings().clone()
        })
    }
}
