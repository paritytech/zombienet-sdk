use serde_json::json;
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

// `genesis_patch` is an RFC 6902 patch applied to the runtime genesis after
// the `genesis` overrides. Overrides replace a list wholesale; a patch can
// append to one the preset already fills, here the funded accounts.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let patch = json!([
        {
            "op": "add",
            "path": "/balances/balances/-",
            "value": ["5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty", 1_000_000_000_000_u64]
        },
        { "op": "replace", "path": "/configuration/config/max_code_size", "value": 2_097_152 },
    ]);

    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_genesis_patch(patch)
                .with_validator(|v| v.with_name("alice"))
                .with_validator(|v| v.with_name("bob"))
        })
        .build()
        .unwrap()
        .spawn_native()
        .await?;

    let alice = network.get_node("alice")?;
    alice
        .wait_metric("block_height{status=\"best\"}", |b| b > 1.0)
        .await?;
    println!("alice is producing blocks on the patched genesis");

    network.destroy().await?;
    Ok(())
}
