use std::{panic::AssertUnwindSafe, path::Path, time::Duration};

use futures::FutureExt;
use tokio::time::timeout;
use zombienet_sdk::{AttachToLive, AttachToLiveNetwork, LocalFileSystem, Network, NetworkNode};

use super::evidence;
use crate::report::{Category, DiagnosticReport, Severity};

const ATTACH_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) async fn attach_network(
    zombie_json_path: &Path,
) -> Result<Network<LocalFileSystem>, anyhow::Error> {
    let zombie_json_path = zombie_json_path.to_path_buf();
    Ok(AttachToLiveNetwork::attach_native(zombie_json_path).await?)
}

/// Attach with a hard timeout and convert panics into errors
/// `Network` is not unwind-safe; `AssertUnwindSafe` is required to call `catch_unwind`
pub(super) async fn attach_network_bounded(
    zombie_json_path: &Path,
) -> Result<Network<LocalFileSystem>, anyhow::Error> {
    match timeout(
        ATTACH_TIMEOUT,
        AssertUnwindSafe(attach_network(zombie_json_path)).catch_unwind(),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => Err(anyhow::anyhow!(
            "network attach panicked: {}",
            panic_message(&panic)
        )),
        Err(_) => Err(anyhow::anyhow!(
            "network attach timed out after {} seconds",
            ATTACH_TIMEOUT.as_secs()
        )),
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = panic.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

/// Attach to the live native network, pushing evidence on failure
pub(super) async fn open_network(
    report: &mut DiagnosticReport,
    zombie_json_path: &Path,
) -> Option<Network<LocalFileSystem>> {
    match attach_network_bounded(zombie_json_path).await {
        Ok(network) => Some(network),
        Err(error) => {
            evidence::push(
                report,
                Severity::Error,
                "network.attach_failed",
                Category::Startup,
                zombie_json_path.display().to_string(),
                "Could not attach to network",
                zombie_json_path.display().to_string(),
                Some(error.to_string()),
            );
            None
        },
    }
}

/// Look up a node by name, pushing `node.{name}.missing` evidence on failure
pub(super) fn lookup_node<'a>(
    report: &mut DiagnosticReport,
    network: &'a Network<LocalFileSystem>,
    node_name: &str,
    source: &Path,
) -> Option<&'a NetworkNode> {
    match network.get_node(node_name) {
        Ok(node) => Some(node),
        Err(error) => {
            evidence::push(
                report,
                Severity::Error,
                format!("node.{node_name}.missing"),
                Category::Liveness,
                node_name.to_string(),
                "Node was not found",
                source.display().to_string(),
                Some(error.to_string()),
            );
            None
        },
    }
}
