use configuration::NetworkConfigBuilder;
use orchestrator::Orchestrator;
use provider::NativeProvider;
use support::fs::local::LocalFileSystem;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
        })
        .build().unwrap();

    println!("{:?}", &config);

    let fs = LocalFileSystem;
    let provider = NativeProvider::new(fs.clone());
    let orchestrator = Orchestrator::new(fs, provider);
    orchestrator.spawn(config).await?;
    while true {

    }
    Ok(())
}
