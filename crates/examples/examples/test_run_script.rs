use std::path::PathBuf;

use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt, RunScriptOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Build a simple network configuration
    let network = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:v1.20.2")
                .with_validator(|node| node.with_name("alice"))
        })
        .build()
        .unwrap()
        .spawn_docker()
        .await?;

    println!("🚀🚀🚀🚀 network deployed");

    let alice = network.get_node("alice")?;

    // Create test script if it doesn't exist
    let script_path = PathBuf::from("./test_script.sh");
    if !script_path.exists() {
        println!("Creating test_script.sh...");
        std::fs::write(
            &script_path,
            r#"#!/bin/bash
                        echo "✅ Script executed successfully!"
                        echo "Arguments: $@"
                        echo "NODE_NAME env: $NODE_NAME"
                        echo "Working dir: $(pwd)"
                        echo "Hostname: $(hostname)"
                        exit 0
                    "#,
        )?;
    }

    println!("Running test_script.sh with args and env vars");
    let options = RunScriptOptions::new(&script_path)
        .args(vec!["arg1".to_string(), "arg2".to_string()])
        .env(vec![("NODE_NAME".to_string(), "alice".to_string())]);

    println!("Executing script...");
    match alice.run_script(options).await? {
        Ok(stdout) => {
            println!("\n✅ Script executed successfully!");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("STDOUT:\n{}", stdout);
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        },
        Err((status, stderr)) => {
            println!("\n❌ Script failed with exit code: {:?}", status.code());
            println!("STDERR:\n{}", stderr);
            return Err("Script execution failed".into());
        },
    }

    network.destroy().await?;

    Ok(())
}
