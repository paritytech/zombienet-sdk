use std::{env, time::Duration};
use zombienet_sdk::{NetworkConfig, environment::Provider};

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Commands
}


#[derive(Subcommand, Debug, Clone)]
enum Commands {
    Spawn {
        config: String,
        #[arg(short, long, value_parser = clap::builder::PossibleValuesParser::new(["docker", "k8s", "native"]),default_value="docker")]
        provider: String
    },
}


#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let (config, provider) = match args.cmd {
        Commands::Spawn { config, provider } => { (config, provider)}
    };

    let config = NetworkConfig::load_from_toml(&config).unwrap();
    let provider: Provider = provider.into();
    let spawn_fn = provider.get_spawn_fn();
    let _n = spawn_fn(config).await.unwrap();

    println!("looping...");

    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
