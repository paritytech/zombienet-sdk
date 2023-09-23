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
        .with_parachain(|p| {
            p.with_id(100)
            .cumulus_based(true)
            .with_collator(|n| {
                n.with_name("collator")
                .with_command("polkadot-parachain")
                //.with_command("adder-collator")
            })
        })
        .build().unwrap();

    println!("{:?}", &config);

    let fs = LocalFileSystem;
    let provider = NativeProvider::new(fs.clone());
    let orchestrator = Orchestrator::new(fs, provider);
    let _network = orchestrator.spawn(config).await?;
    //println!("{:#?}", network);
    while true {

    }
    Ok(())
}
