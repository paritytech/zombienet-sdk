use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use futures::StreamExt;
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt};

fn find_keystore_dir(node_dir: &Path) -> Option<PathBuf> {
    let data_chains = node_dir.join("data").join("chains");
    if data_chains.exists() {
        if let Ok(entries) = std::fs::read_dir(&data_chains) {
            for entry in entries.flatten() {
                let keystore = entry.path().join("keystore");
                if keystore.exists() && keystore.is_dir() {
                    return Some(keystore);
                }
            }
        }
    }
    None
}

fn extract_key_type_hex(filename: &str) -> Option<&str> {
    if filename.len() >= 8 {
        Some(&filename[..8])
    } else {
        None
    }
}

fn get_keystore_key_types(keystore_path: &Path) -> std::io::Result<HashSet<String>> {
    let mut key_types = HashSet::new();

    if keystore_path.exists() {
        for entry in std::fs::read_dir(keystore_path)? {
            let entry = entry?;
            let filename = entry.file_name().to_string_lossy().to_string();
            if let Some(hex_prefix) = extract_key_type_hex(&filename) {
                key_types.insert(hex_prefix.to_string());
            }
        }
    }

    Ok(key_types)
}

fn verify_keystore_keys(
    keystore_path: &Path,
    expected_key_types: &[&str],
    node_name: &str,
) -> Result<(), String> {
    let expected_hex: HashSet<String> = expected_key_types.iter().map(hex::encode).collect();

    let actual_hex = get_keystore_key_types(keystore_path)
        .map_err(|e| format!("Failed to read keystore for {}: {}", node_name, e))?;

    if expected_hex != actual_hex {
        return Err(format!("Keystore mismatch for {}:\n", node_name,));
    }

    Ok(())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.20.2")
                .with_validator(|node| {
                    node.with_name("alice")
                        .with_keystore_key_types(vec!["aura", "gran", "babe"])
                })
                .with_validator(|node| {
                    node.with_name("bob")
                        .with_keystore_key_types(vec!["aura", "gran_sr", "babe", "imon"])
                })
                .with_validator(|node| {
                    node.with_name("charlie")
                        .with_keystore_key_types(vec!["aura", "gran", "cust_sr", "myky_ec"])
                })
                .with_validator(|node| node.with_name("dave"))
        })
        .build()
        .unwrap()
        .spawn_docker()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let base_dir = network
        .base_dir()
        .ok_or("Failed to get network base directory")?;

    println!("📁 Network base directory: {}", base_dir);
    println!();

    println!("🔑 Verifying keystore key types...");
    println!();

    let node_expectations = [
        ("alice", vec!["aura", "gran", "babe"]),
        ("bob", vec!["aura", "gran", "babe", "imon"]), // gran_sr still creates "gran" key type
        ("charlie", vec!["aura", "gran", "cust", "myky"]),
    ];

    for (node_name, expected_keys) in &node_expectations {
        let node_dir = PathBuf::from(base_dir).join(node_name);
        let keystore_path = find_keystore_dir(&node_dir);

        if let Some(path) = keystore_path {
            println!("{} keystore: {}", node_name, path.display());

            match verify_keystore_keys(&path, expected_keys, node_name) {
                Ok(()) => {
                    println!("    ✅ Verified: {:?}", expected_keys);
                },
                Err(err) => {
                    println!("    ❌ {}", err);
                },
            }
        }
    }

    let alice = network.get_node("alice")?;
    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;
    let mut finalized_blocks = client.blocks().subscribe_finalized().await?.take(3);

    while let Some(block) = finalized_blocks.next().await {
        println!("Finalized block {}", block?.header().number);
    }

    network.destroy().await?;

    Ok(())
}
