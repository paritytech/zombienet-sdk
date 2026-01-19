use std::path::PathBuf;

use futures::stream::StreamExt;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};
use zombienet_sdk::{subxt, NetworkConfigBuilder, NetworkConfigExt};

// Account constants
const ALICE: &str = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
const BOB: &str = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";

// Balance constants (match the example post-process script / plain spec)
const ALICE_BALANCE: u64 = 5_000_000_000_000_000_000;
const BOB_BALANCE: u64 = 2_000_000_000_000;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let script_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/customize-chain-spec.sh"
    );

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
                .with_post_process_script(script_path)
        })
        .with_parachain(|p| {
            p.with_id(2000)
                .with_chain("asset-hub-rococo-local")
                .with_default_command("polkadot-parachain")
                .with_collator(|c| c.with_name("collator"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await
        .unwrap();

    println!("\n🚀 Network deployed successfully!");
    println!("📝 Chain-spec has been post-processed by the script");

    // Verify the chain-spec JSON was actually modified (search multiple likely locations)
    println!("\n🔍 Verifying chain-spec modification...");
    let base_dir = network.base_dir().expect("Network should have a base_dir");
    let relay_chain_spec_path =
        PathBuf::from(base_dir).join(format!("{}-plain.json", network.relaychain().chain()));

    if relay_chain_spec_path.exists() {
        if let Ok(spec_content) = std::fs::read_to_string(&relay_chain_spec_path) {
            let spec: serde_json::Value = serde_json::from_str(&spec_content)?;

            println!(
                "Reading generated relay chain-spec from: {}",
                relay_chain_spec_path.display()
            );
            assert!(verify_balances(&spec));
            println!("Verifications passed!");
        } else {
            println!("Could not read chain-spec file for verification");
        }
    } else {
        println!(
            "Chain-spec file not found at: {}",
            relay_chain_spec_path.display()
        );
    }

    println!("\n⏳ Waiting for network to produce blocks...");
    let alice = network.get_node("alice")?;
    let client = alice.wait_client::<subxt::PolkadotConfig>().await?;

    // Wait for a few blocks to be produced
    let mut blocks = client.blocks().subscribe_finalized().await?.take(3);
    println!("\n📦 Finalized blocks:");
    while let Some(block) = blocks.next().await {
        println!("  Block #{}", block?.header().number);
    }

    Ok(())
}

/// Verify that balances were correctly added
fn verify_balances(spec: &serde_json::Value) -> bool {
    let balances_paths = vec![
        "/genesis/runtime/balances/balances",
        "/genesis/runtimeGenesis/patch/balances/balances",
    ];

    let mut found_balances = None;
    for path in balances_paths {
        if let Some(balances) = spec.pointer(path) {
            found_balances = Some(balances);
            break;
        }
    }

    if let Some(balances) = found_balances {
        if let Some(balances_array) = balances.as_array() {
            let expected_balances = vec![(ALICE, ALICE_BALANCE), (BOB, BOB_BALANCE)];

            for (target_account, target_balance) in expected_balances {
                let found = balances_array.iter().any(|entry| {
                    if let Some(arr) = entry.as_array() {
                        if arr.len() >= 2 {
                            let account = arr[0].as_str().unwrap_or("");
                            let balance = arr[1].as_u64().unwrap_or(0);
                            account == target_account && balance == target_balance
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                });

                if found {
                    println!(
                        "  CustomDecorator applied: {} has {} tokens",
                        target_account, target_balance
                    );
                } else {
                    println!(" Balance NOT found for: {}", target_account);
                    return false;
                }
            }
        }
    } else {
        println!("Balances section NOT found in chain-spec at known paths");
        return false;
    }

    true
}
