use std::time::Duration;

use tokio::time::timeout;

use crate::network::node::NetworkNode;

pub async fn verify_nodes(nodes: &[&NetworkNode]) -> Result<(), anyhow::Error> {
    timeout(Duration::from_secs(90), check_nodes(nodes))
        .await
        .map_err(|_| anyhow::anyhow!("one or more nodes are not ready!"))
}

// TODO: we should inject in someway the logic to make the request
// in order to allow us to `mock` and easily test this.
// maybe moved to the provider with a NodeStatus, and some helpers like wait_running, wait_ready, etc... ? to be discussed
async fn check_nodes(nodes: &[&NetworkNode]) {
    loop {
        let tasks: Vec<_> = nodes
            .iter()
            .map(|node| {
                // TODO: move to logger
                // println!("getting from {}", node.name);
                reqwest::get(node.prometheus_uri.clone())
            })
            .collect();

        let all_ready = futures::future::try_join_all(tasks).await;
        if all_ready.is_ok() {
            return;
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
