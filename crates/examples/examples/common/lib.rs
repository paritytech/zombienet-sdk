use std::path::Path;

use zombienet_sdk::{NetworkConfig, NetworkConfigBuilder};

#[allow(dead_code)]
pub fn small_network_config(
    custom_base_dir: Option<&Path>,
) -> Result<NetworkConfig, Vec<anyhow::Error>> {
    // let config =
    let builder = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(2000).cumulus_based(true).with_collator(|n| {
                n.with_name("collator")
                    .with_command("polkadot-parachain")
                    .with_image("docker.io/parity/polkadot-parachain:latest")
            })
        });

    if let Some(base_dir) = custom_base_dir {
        builder
            .with_global_settings(|g| g.with_base_dir(base_dir))
            .build()
    } else {
        builder.build()
    }
}
