use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use configuration::{
    shared::constants::THIS_IS_A_BUG, GlobalSettings, HrmpChannelConfig, NetworkConfig,
};
use futures::future::try_join_all;
use provider::{ProviderError, ProviderNamespace};
use tracing::debug;

use crate::errors::OrchestratorError;

pub mod node;
pub mod parachain;
pub mod relaychain;

use self::{node::NodeSpec, parachain::ParachainSpec, relaychain::RelaychainSpec};

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

    pub async fn populate_nodes_available_args(
        &mut self,
        ns: Arc<dyn ProviderNamespace + Send + Sync>,
    ) -> Result<(), OrchestratorError> {
        let network_nodes = self.collect_network_nodes();

        let mut image_command_to_nodes_mapping =
            Self::create_image_command_to_nodes_mapping(network_nodes);

        let available_args_outputs =
            Self::retrieve_all_nodes_available_args_output(ns, &image_command_to_nodes_mapping)
                .await?;

        Self::update_nodes_available_args_output(
            &mut image_command_to_nodes_mapping,
            available_args_outputs,
        );

        Ok(())
    }

    //
    pub async fn node_available_args_output(
        &self,
        node_spec: &NodeSpec,
        ns: Arc<dyn ProviderNamespace + Send + Sync>,
    ) -> Result<String, ProviderError> {
        // try to find a node that use the same combination of image/cmd
        let cmp_fn = |ad_hoc: &&NodeSpec| -> bool {
            ad_hoc.image == node_spec.image && ad_hoc.command == node_spec.command
        };

        // check if we already had computed the args output for this cmd/[image]
        let node = self.relaychain.nodes.iter().find(cmp_fn);
        let node = if let Some(node) = node {
            Some(node)
        } else {
            let node = self
                .parachains
                .iter()
                .find_map(|para| para.collators.iter().find(cmp_fn));

            node
        };

        let output = if let Some(node) = node {
            node.available_args_output.clone().expect(&format!(
                "args_output should be set for running nodes {THIS_IS_A_BUG}"
            ))
        } else {
            // we need to compute the args output
            let image = node_spec
                .image
                .as_ref()
                .map(|image| image.as_str().to_string());
            let command = node_spec.command.as_str().to_string();

            ns.get_node_available_args((command, image)).await?
        };

        Ok(output)
    }

    // collect mutable references to all nodes from relaychain and parachains
    fn collect_network_nodes(&mut self) -> Vec<&mut NodeSpec> {
        vec![
            self.relaychain.nodes.iter_mut().collect::<Vec<_>>(),
            self.parachains
                .iter_mut()
                .flat_map(|para| para.collators.iter_mut())
                .collect(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
    }

    // initialize the mapping of all possible node image/commands to corresponding nodes
    fn create_image_command_to_nodes_mapping(
        network_nodes: Vec<&mut NodeSpec>,
    ) -> HashMap<(Option<String>, String), Vec<&mut NodeSpec>> {
        network_nodes.into_iter().fold(
            HashMap::new(),
            |mut acc: HashMap<(Option<String>, String), Vec<&mut node::NodeSpec>>, node| {
                // build mapping key using image and command if image is present or command only
                let key = node
                    .image
                    .as_ref()
                    .map(|image| {
                        (
                            Some(image.as_str().to_string()),
                            node.command.as_str().to_string(),
                        )
                    })
                    .unwrap_or_else(|| (None, node.command.as_str().to_string()));

                // append the node to the vector of nodes for this image/command tuple
                if let Entry::Vacant(entry) = acc.entry(key.clone()) {
                    entry.insert(vec![node]);
                } else {
                    acc.get_mut(&key).unwrap().push(node);
                }

                acc
            },
        )
    }

    async fn retrieve_all_nodes_available_args_output(
        ns: Arc<dyn ProviderNamespace + Send + Sync>,
        image_command_to_nodes_mapping: &HashMap<(Option<String>, String), Vec<&mut NodeSpec>>,
    ) -> Result<Vec<(Option<String>, String, String)>, OrchestratorError> {
        try_join_all(
            image_command_to_nodes_mapping
                .keys()
                .cloned()
                .map(|(image, command)| async {
                    // get node available args output from image/command
                    let available_args = ns
                        .get_node_available_args((command.clone(), image.clone()))
                        .await?;
                    debug!(
                        "retrieved available args for image: {:?}, command: {}",
                        image, command
                    );

                    // map the result to include image and command
                    Ok::<_, OrchestratorError>((image, command, available_args))
                })
                .collect::<Vec<_>>(),
        )
        .await
    }

    fn update_nodes_available_args_output(
        image_command_to_nodes_mapping: &mut HashMap<(Option<String>, String), Vec<&mut NodeSpec>>,
        available_args_outputs: Vec<(Option<String>, String, String)>,
    ) {
        for (image, command, available_args_output) in available_args_outputs {
            let nodes = image_command_to_nodes_mapping
                .get_mut(&(image, command))
                .expect(&format!(
                    "node image/command key should exist {THIS_IS_A_BUG}"
                ));

            for node in nodes {
                node.available_args_output = Some(available_args_output.clone());
            }
        }
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
