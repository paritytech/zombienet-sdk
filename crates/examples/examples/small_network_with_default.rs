use std::time::Duration;

use configuration::{NetworkConfigBuilder};
use orchestrator::{AddNodeOptions, Orchestrator};
use provider::NativeProvider;
use support::fs::local::LocalFileSystem;
use orchestrator::validator_actions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_node(|node| node.with_name("alice"))
                .with_node(|node| node.with_name("bob"))
                .with_node(|node| node.with_name("charlie"))
        })
        .with_parachain(|p| {
            p.with_id(100)
                .cumulus_based(true)
                .with_collator(|n| n.with_name("collator").with_command("polkadot-parachain"))
        })
        .build()
        .unwrap();

    let fs = LocalFileSystem;
    // let provider = NativeProvider::new(fs.clone());
    let provider = NativeProvider::new(LocalFileSystem.clone());
    let orchestrator = Orchestrator::new(fs, provider);
    let mut network = orchestrator.spawn(config).await?;


    println!("🚀🚀🚀🚀 network deployed");

    // let relay = network.relaychain_mut();
    // relay.a(String::from("b"));

    // let a = network.relaychain().chain();
    // println!("{:?}", a);


    // add  a new node
    let opts = AddNodeOptions {
        rpc_port: Some(9444),
        is_validator: false,
        ..Default::default()
    };

    // TODO: add check to ensure if unique
    network.add_node("dave", opts).await?;

    // // Example of some opertions that you can do
    // // with `nodes` (e.g pause, resume, restart)

    // // Get a ref to the node
    // let dave = network.get_node("dave")?;
    // let alice = network.get_node("alice")?;

    // // let is_10 = node.assert("block_height{status=\"best\"}", 10).await?;
    // // println!("is_10: {is_10}");

    // let role = dave.reports("node_roles").await?;
    // println!("Role is {}", role);

    // let para_validator = dave.reports("polkadot_node_is_parachain_validator").await?;
    // println!("para_validator is {para_validator}");

    // let node_validator = dave.reports("polkadot_node_is_active_validator").await?;
    // println!("node_is_active_validator is {node_validator}");

    // // deregister and check
    // //let pk = dave.
    // validator_actions::deregister(vec![&dave], alice.ws_uri(), None).await?;
    // // alice: js-script ./0003-deregister-register-validator.js with "deregister,dave" return is 0 within 30 secs

    // // # Wait 2 sessions. The authority set change is enacted at curent_session + 2.
    // // sleep 120 seconds
    // tokio::time::sleep(Duration::from_secs(180)).await;
    // // dave: reports polkadot_node_is_parachain_validator is 0 within 180 secs
    // // dave: reports polkadot_node_is_active_validator is 0 within 180 secs
    // let para_validator = dave.reports("polkadot_node_is_parachain_validator").await?;
    // println!("para_validator is {para_validator}");

    // let node_validator = dave.reports("polkadot_node_is_active_validator").await?;
    // println!("node_is_active_validator is {node_validator}");

    // // re-register
    // validator_actions::register(vec![&dave], alice.ws_uri(), None).await?;

    // tokio::time::sleep(Duration::from_secs(180)).await;

    // let para_validator = dave.reports("polkadot_node_is_parachain_validator").await?;
    // println!("para_validator is {para_validator}");

    // let node_validator = dave.reports("polkadot_node_is_active_validator").await?;
    // println!("node_is_active_validator is {node_validator}");



    // // pause the node
    // // node.pause().await?;
    // // println!("node new1 paused!");

    // // node.resume().await?;
    // // println!("node new1 resumed!");

    // let col_opts = AddNodeOpts {
    //     command: Some("polkadot-parachain".try_into()?),
    //     ..Default::default()
    // };
    // // network.add_node("new-col-1", col_opts, Some(100)).await?;
    // // println!("new collator deployed!");

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

    // Ok(())
}
