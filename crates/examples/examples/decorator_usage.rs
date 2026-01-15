use std::{path::PathBuf, sync::Arc};

use configuration::{NetworkConfig, NetworkConfigBuilder};
use orchestrator::{
    decorators::{ChainSpecDecorator, DecoratorRegistry},
    Orchestrator,
};
use provider::NativeProvider;
use zombienet_sdk::LocalFileSystem;

// Account constants
const ALICE: &str = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
const BOB: &str = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";
const CHARLIE: &str = "5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y";
const BAD_BLOCKS: &str = "0x53849a2121fe81fde85859dcebe8cc9c37791c01a9702ce65615b1dcb8ac53e5";

// Balance constants
const ALICE_BALANCE: u64 = 42_000_000_000_000_000;
const BOB_BALANCE: u64 = 10_000_000_000_000_000;
const CHARLIE_BALANCE: u64 = 5_000_000_000_000_000;

// Genesis paths
const BALANCES_PATH: &str = "/genesis/runtimeGenesis/patch/balances/balances";
const SUDO_KEY_PATH: &str = "/genesis/runtimeGenesis/patch/sudo/key";
const BAD_BLOCKS_PATH: &str = "/badBlocks";

/// Example custom decorator that implements the ChainSpecDecorator trait
struct CustomDecorator {
    balances: Vec<(String, u64)>,
    bad_blocks: Vec<String>,
}

impl CustomDecorator {
    fn new(balances: Vec<(String, u64)>, bad_blocks: Vec<String>) -> Self {
        Self {
            balances,
            bad_blocks,
        }
    }
}

impl ChainSpecDecorator for CustomDecorator {
    fn name(&self) -> &str {
        "custom"
    }

    fn customize_relay(&self, spec: &mut serde_json::Value) -> Result<(), anyhow::Error> {
        let array_mut = if let Some(node) = spec.pointer_mut(BAD_BLOCKS_PATH) {
            if node.is_null() {
                *node = serde_json::json!([]);
            }
            node.as_array_mut()
        } else {
            spec["badBlocks"] = serde_json::json!([]);
            spec.pointer_mut(BAD_BLOCKS_PATH)
                .and_then(|n| n.as_array_mut())
        };

        if let Some(array) = array_mut {
            for account in &self.bad_blocks {
                array.push(serde_json::json!([account, 1]));
            }
            println!("badBlocks updated");
        } else {
            println!(
                "Could not create or access badBlocks array at path: {}",
                BAD_BLOCKS_PATH
            );
        }

        Ok(())
    }

    fn customize_para(&self, _spec: &mut serde_json::Value) -> Result<(), anyhow::Error> {
        Ok(())
    }

    // Override the specialized add_balances method
    fn add_balances(&self, spec: &mut serde_json::Value) -> Option<Result<(), anyhow::Error>> {
        if let Some(balances) = spec.pointer_mut(BALANCES_PATH) {
            if let Some(balances_array) = balances.as_array_mut() {
                for (account, balance) in &self.balances {
                    // Check if account already exists
                    let existing_index = balances_array.iter().position(|entry| {
                        if let Some(arr) = entry.as_array() {
                            if !arr.is_empty() {
                                arr[0].as_str() == Some(account.as_str())
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    });

                    if let Some(index) = existing_index {
                        // Update existing balance
                        balances_array[index] = serde_json::json!([account, balance]);
                        println!("Updated balance: {} -> {}", account, balance);
                    } else {
                        // Add new balance
                        balances_array.push(serde_json::json!([account, balance]));
                        println!("Added new balance: {} -> {}", account, balance);
                    }
                }
            }
        }

        Some(Ok(()))
    }
}

/// Example decorator that modifies the sudo key
struct CustomSudoDecorator {
    sudo_key: String,
}

impl CustomSudoDecorator {
    fn new(sudo_key: String) -> Self {
        Self { sudo_key }
    }
}

impl ChainSpecDecorator for CustomSudoDecorator {
    fn name(&self) -> &str {
        "custom_sudo"
    }

    fn customize_relay(&self, spec: &mut serde_json::Value) -> Result<(), anyhow::Error> {
        if let Some(sudo) = spec.pointer_mut(SUDO_KEY_PATH) {
            *sudo = serde_json::json!(self.sudo_key);
            println!("Set sudo key to: {}", self.sudo_key);
        }
        Ok(())
    }

    fn customize_para(&self, spec: &mut serde_json::Value) -> Result<(), anyhow::Error> {
        if let Some(sudo) = spec.pointer_mut(SUDO_KEY_PATH) {
            *sudo = serde_json::json!(self.sudo_key);
            println!("Set sudo key to: {}", self.sudo_key);
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Build network configuration
    let config = create_network_config()?;

    // Create decorator registry with multiple decorators
    let registry = create_decorator_registry();
    println!("Registered decorators: {:?}", registry.names());

    // Spawn network with decorators - decorators will be applied during chain-spec generation
    println!("Spawning network with custom decorators...");

    let filesystem = LocalFileSystem {};
    let provider = NativeProvider::new(filesystem.clone());
    let network = Orchestrator::new(filesystem, provider)
        .with_customizer(registry)
        .spawn(config)
        .await?;

    println!("🚀🚀🚀🚀 Network spawned successfully with custom chain-spec decorators!");

    // Get the network base directory
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
            assert!(verify_sudo_key(&spec));
            assert!(verify_bad_blocks(&spec));

            println!("All decorator verifications passed!");
        } else {
            println!("Could not read chain-spec file for verification");
        }
    } else {
        println!(
            "Chain-spec file not found at: {}",
            relay_chain_spec_path.display()
        );
    }

    println!("Network directory: {}", base_dir);

    Ok(())
}

/// Create sample network configuration
fn create_network_config() -> Result<NetworkConfig, Box<dyn std::error::Error>> {
    NetworkConfigBuilder::new()
        .with_relaychain(|relaychain| {
            relaychain
                .with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|parachain| {
            parachain
                .with_id(2000)
                .with_chain("asset-hub-rococo-local")
                .with_default_command("polkadot-parachain")
                .with_collator(|collator| collator.with_name("collator-01"))
        })
        .build()
        .map_err(|e| format!("Failed to build config: {:?}", e).into())
}

/// Create and configure decorator registry
fn create_decorator_registry() -> DecoratorRegistry {
    let mut registry = DecoratorRegistry::new();

    // Register custom decorator with multiple balances
    let custom_decorator = Arc::new(CustomDecorator::new(
        vec![
            (ALICE.to_string(), ALICE_BALANCE),
            (BOB.to_string(), BOB_BALANCE),
            (CHARLIE.to_string(), CHARLIE_BALANCE),
        ],
        vec![BAD_BLOCKS.to_string()],
    ));
    registry.register(custom_decorator);
    println!("Registered custom decorator");

    // Register sudo decorator
    let sudo_decorator = Arc::new(CustomSudoDecorator::new(CHARLIE.to_string()));
    registry.register(sudo_decorator);
    println!("Registered sudo decorator");

    registry
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
            let expected_balances = vec![
                (ALICE, ALICE_BALANCE),
                (BOB, BOB_BALANCE),
                (CHARLIE, CHARLIE_BALANCE),
            ];

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

/// Verify that sudo key was correctly changed
fn verify_sudo_key(spec: &serde_json::Value) -> bool {
    let sudo_paths = vec![
        "/genesis/runtime/sudo/key",
        "/genesis/runtimeGenesis/patch/sudo/key",
    ];

    let mut found_sudo = None;
    for path in sudo_paths {
        if let Some(sudo_key) = spec.pointer(path) {
            found_sudo = Some(sudo_key);
            break;
        }
    }

    if let Some(sudo_key) = found_sudo {
        if let Some(key_str) = sudo_key.as_str() {
            if key_str == CHARLIE {
                println!("  CustomSudoDecorator applied: sudo key is {}", CHARLIE);
                return true;
            } else {
                println!(
                    "  Sudo key mismatch! Expected: {}, Found: {} (decorator may not have been applied)",
                    CHARLIE, key_str
                );
            }
        }
    } else {
        println!("Sudo key NOT found in chain-spec at known paths");
    }

    false
}

/// Verify that bad blocks were correctly added
fn verify_bad_blocks(spec: &serde_json::Value) -> bool {
    if let Some(bad_blocks) = spec.pointer(BAD_BLOCKS_PATH) {
        if let Some(bad_blocks_array) = bad_blocks.as_array() {
            let found = bad_blocks_array.iter().any(|entry| {
                if let Some(arr) = entry.as_array() {
                    if !arr.is_empty() {
                        let account = arr[0].as_str().unwrap_or("");
                        account == BAD_BLOCKS
                    } else {
                        false
                    }
                } else {
                    false
                }
            });

            if found {
                println!("CustomDecorator applied: bad blocks found");
                return true;
            } else {
                println!("Bad blocks NOT found");
            }
        }
    } else {
        println!("Bad blocks section NOT found in chain-spec at known paths");
    }

    false
}
