use std::path::Path;

use zombienet_sdk::NetworkConfigExt;

#[path = "./common/lib.rs"]
mod common;


#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let config = common::small_network_config(Some(Path::new("/tmp/zombie-1"))).unwrap();
    let _network = config.spawn_docker().await.unwrap();

    // For now let just loop....
    #[allow(clippy::empty_loop)]
    loop {}

}