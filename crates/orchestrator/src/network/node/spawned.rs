//! The erased node interface.
//!
//! [`SpawnedNode`] is what lets a [`Network`](crate::network::Network) keep
//! every node it spawned — substrate and JAM alike — in a single registry,
//! while still handing back the concrete type on request.
//!
//! Downcasting relies on trait upcasting (`&dyn SpawnedNode -> &dyn Any`),
//! stable since Rust 1.86, which is the workspace MSRV. That's why there is
//! no `as_any()` method here.

use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::core::NodeCore;

/// Which flavour of node a [`SpawnedNode`] is.
///
/// `Any` gives no type information on a failed downcast, so nodes carry this
/// tag for readable errors and for filtering the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A substrate node: relaychain node or collator.
    Substrate,
    /// A JAM node.
    Jam,
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeKind::Substrate => write!(f, "substrate"),
            NodeKind::Jam => write!(f, "jam"),
        }
    }
}

/// Behaviour shared by every node zombienet spawns, regardless of what it runs.
///
/// Implementors only need to provide [`core`](SpawnedNode::core) and
/// [`wait_until_is_up`](SpawnedNode::wait_until_is_up); everything else is
/// delegated to the [`NodeCore`].
///
/// NOTE: this trait is deliberately dyn-compatible. Generic methods (e.g the
/// subxt `client::<Config>()`) stay inherent on the concrete types — downcast
/// with [`Network::get_node`](crate::network::Network::get_node) or
/// [`Network::get_jam_node`](crate::network::Network::get_jam_node) to reach them.
#[async_trait]
pub trait SpawnedNode: std::any::Any + erased_serde::Serialize + Send + Sync + 'static {
    /// The provider handle and runtime state of this node.
    fn core(&self) -> &NodeCore;

    /// Wait until the node reports it finished booting.
    ///
    /// How readiness is established is up to each node flavour (Prometheus
    /// scrape for substrate, log/rpc probe for JAM), which is why this has no
    /// default implementation.
    async fn wait_until_is_up(&self, timeout_secs: u64) -> Result<(), anyhow::Error>;

    /// What flavour of node this is.
    fn kind(&self) -> NodeKind {
        self.core().kind()
    }

    fn name(&self) -> &str {
        self.core().name()
    }

    fn is_running(&self) -> bool {
        self.core().is_running()
    }

    fn last_start_ts(&self) -> u64 {
        self.core().last_start_ts()
    }

    fn base_dir(&self) -> &PathBuf {
        self.core().base_dir()
    }

    fn args(&self) -> Vec<&str> {
        self.core().args()
    }

    async fn logs(&self) -> Result<String, anyhow::Error> {
        self.core().logs().await
    }

    async fn pause(&self) -> Result<(), anyhow::Error> {
        self.core().pause().await
    }

    async fn resume(&self) -> Result<(), anyhow::Error> {
        self.core().resume().await
    }

    async fn restart(&self, after: Option<Duration>) -> Result<(), anyhow::Error> {
        self.core().restart(after).await
    }
}

erased_serde::serialize_trait_object!(SpawnedNode);
