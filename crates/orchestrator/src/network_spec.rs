use configuration::{GlobalSettings, HrmpChannelConfig, NetworkConfig};

use crate::errors::OrchestratorError;

pub mod node;
pub mod parachain;
pub mod relaychain;

use self::{parachain::ParachainSpec, relaychain::RelaychainSpec};

#[derive(Debug, Clone)]
pub struct NetworkSpec {
    /// Relaychain configuration.
    pub(crate) relaychain: RelaychainSpec,

    /// Parachains configurations.
    pub(crate) parachains: Vec<ParachainSpec>,

    /// HRMP channels configurations.
    pub(crate) hrmp_channels: Vec<HrmpChannelConfig>,

    /// Global settings
    pub(crate) global_settings: GlobalSettings,
}

impl NetworkSpec {
    pub async fn from_config(
        network_config: &NetworkConfig,
    ) -> Result<NetworkSpec, OrchestratorError> {
        let mut errs = vec![];
        let relaychain = RelaychainSpec::from_config(network_config.relaychain())?;
        let mut parachains = vec![];

        // TODO: move to `fold` or map+fold
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
            global_settings: network_config.global_settings().clone(),
        })
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn small_network_config_get_spec() {
        use configuration::NetworkConfigBuilder;

        use super::*;

        let config = NetworkConfigBuilder::new()
            .with_relaychain(|r| {
                r.with_chain("rococo-local")
                    .with_default_command("polkadot")
                    .with_node(|node| node.with_name("alice"))
                    .with_node(|node| {
                        node.with_name("bob")
                            .with_command("polkadot1")
                            .validator(false)
                    })
            })
            .with_parachain(|p| {
                p.with_id(100)
                    .with_default_command("adder-collator")
                    .with_collator(|c| c.with_name("collator1"))
            })
            .build()
            .unwrap();

        let network_spec = NetworkSpec::from_config(&config).await.unwrap();
        let alice = network_spec.relaychain.nodes.first().unwrap();
        let bob = network_spec.relaychain.nodes.get(1).unwrap();
        assert_eq!(alice.command.as_str(), "polkadot");
        assert_eq!(bob.command.as_str(), "polkadot1");
        assert!(alice.is_validator);
        assert!(!bob.is_validator);

        // paras
        assert_eq!(network_spec.parachains.len(), 1);
        let para_100 = network_spec.parachains.first().unwrap();
        assert_eq!(para_100.id, 100);
    }
}
