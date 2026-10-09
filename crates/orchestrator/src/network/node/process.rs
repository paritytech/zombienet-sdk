//! Custom processes: the services a network config declares under
//! `[[custom_processes]]`, running next to the nodes.
//!
//! A [`CustomProcessNode`] is a member of the network like any other node:
//! it is tracked on [`Network`](crate::network::Network), written to
//! `zombie.json`, recreated on attach and destroyed with the namespace. It
//! has no chain, so the only protocol-level surface it exposes is the set of
//! ports it declared and where each one is reachable.

use std::{collections::BTreeMap, net::IpAddr, time::Duration};

use async_trait::async_trait;
use configuration::{types::Port, CustomProcess};
use serde::{Deserialize, Serialize};
use support::net::wait_tcp_ready;
use tracing::debug;

use super::{
    core::NodeCore,
    spawned::{NodeKind, SpawnedNode},
};

/// Where one declared port of a custom process is reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessPort {
    /// The port the process listens on: as declared, or the one picked for `0`.
    pub port: Port,
    /// `host:port` as reachable from where zombienet runs: a port-forward in
    /// k8s (unless running in CI, where it is the pod's own address), the
    /// published host port on docker, the port itself on native.
    pub external: String,
    /// `host:port` as reachable from inside the network (other nodes and
    /// processes): the process's Service in k8s (`<name>:<port>`), the
    /// container ip on docker, the same as `external` on native.
    pub internal: String,
}

/// A running custom process.
#[derive(Clone, Serialize)]
pub struct CustomProcessNode {
    #[serde(flatten)]
    pub(crate) core: NodeCore,
    pub(crate) spec: CustomProcess,
    /// Ip the process is reachable at.
    pub(crate) ip: IpAddr,
    /// Where each declared port is reachable, by the port's name.
    pub(crate) ports: BTreeMap<String, ProcessPort>,
}

/// Deserialization counterpart used when re-attaching to a running network.
#[derive(Deserialize)]
pub(crate) struct RawCustomProcessNode {
    pub(crate) name: String,
    pub(crate) spec: CustomProcess,
    pub(crate) ip: IpAddr,
    #[serde(default)]
    pub(crate) ports: BTreeMap<String, ProcessPort>,
    /// The provider's own record of the process, which it needs to reattach.
    pub(crate) inner: serde_json::Value,
}

impl CustomProcessNode {
    pub(crate) fn new(
        name: impl Into<String>,
        inner: provider::DynNode,
        spec: CustomProcess,
        ip: IpAddr,
        ports: BTreeMap<String, ProcessPort>,
    ) -> Self {
        Self {
            core: NodeCore::new(name, inner, NodeKind::CustomProcess),
            spec,
            ip,
            ports,
        }
    }

    /// The provider-generic part of this process.
    pub fn core(&self) -> &NodeCore {
        &self.core
    }

    pub fn name(&self) -> &str {
        self.core.name()
    }

    /// The configuration this process was spawned from.
    pub fn spec(&self) -> &CustomProcess {
        &self.spec
    }

    /// Ip the process is reachable at.
    pub fn ip(&self) -> IpAddr {
        self.ip
    }

    /// Where every declared port is reachable, by the port's name.
    pub fn ports(&self) -> &BTreeMap<String, ProcessPort> {
        &self.ports
    }

    /// Where the port named `name` is reachable, if the process declared it.
    pub fn port(&self, name: &str) -> Option<&ProcessPort> {
        self.ports.get(name)
    }

    /// `host:port` to reach the port named `name` from where zombienet runs,
    /// if the process declared it.
    pub fn get_uri_for_name(&self, name: &str) -> Option<&str> {
        self.ports.get(name).map(|p| p.external.as_str())
    }

    /// The first declared port, in declaration order, if any.
    fn first_port(&self) -> Option<&ProcessPort> {
        self.spec
            .ports()
            .first()
            .and_then(|declared| self.ports.get(&declared.name))
    }

    /// Check if the process is responsive by connecting to its first declared
    /// port, with a short timeout (2 seconds). A process that declares no port
    /// has nothing to probe and is reported responsive.
    ///
    /// On kubernetes outside CI the address is a local port-forward, and on
    /// docker a published port behind docker's proxy; both accept a connection
    /// whether or not the process listens, so there the probe says the forward
    /// is up, not that the process answers. Native and kubernetes in CI reach
    /// the process itself.
    pub async fn is_responsive(&self) -> bool {
        match self.first_port() {
            Some(port) => {
                tokio::time::timeout(Duration::from_secs(2), wait_tcp_ready(&port.external))
                    .await
                    .is_ok()
            },
            None => true,
        }
    }
}

#[async_trait]
impl SpawnedNode for CustomProcessNode {
    fn core(&self) -> &NodeCore {
        &self.core
    }

    /// A custom process has no readiness definition yet (zombienet-sdk#596):
    /// a declared port that is slow to open must not fail a network-wide
    /// wait, so there is nothing to wait on. [`Self::is_responsive`] probes.
    async fn wait_until_is_up(&self, _timeout_secs: u64) -> Result<(), anyhow::Error> {
        debug!("[{}] custom process, not waited on", self.name());
        Ok(())
    }
}

impl std::fmt::Debug for CustomProcessNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomProcessNode")
            .field("inner", &"inner_skipped")
            .field("name", &self.name())
            .field("spec", &self.spec)
            .field("ip", &self.ip)
            .field("ports", &self.ports)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raw_process_without_ports_still_deserializes_but_needs_its_inner() {
        let json = serde_json::json!({
            "name": "eth-rpc",
            "spec": { "name": "eth-rpc", "command": "eth-rpc" },
            "ip": "127.0.0.1",
            "inner": { "provider_tag": "k8s" },
        });
        let raw: RawCustomProcessNode = serde_json::from_value(json).unwrap();
        assert_eq!(raw.name, "eth-rpc");
        assert!(raw.ports.is_empty());
        assert_eq!(raw.inner["provider_tag"], "k8s");

        let without_inner = serde_json::json!({
            "name": "eth-rpc",
            "spec": { "name": "eth-rpc", "command": "eth-rpc" },
            "ip": "127.0.0.1",
        });
        assert!(serde_json::from_value::<RawCustomProcessNode>(without_inner).is_err());
    }

    #[test]
    fn ports_round_trip_by_name() {
        let ports = BTreeMap::from([(
            "http".to_string(),
            ProcessPort {
                port: 8545,
                external: "127.0.0.1:53421".into(),
                internal: "10.0.0.7:8545".into(),
            },
        )]);
        let json = serde_json::to_value(&ports).unwrap();
        assert_eq!(json["http"]["port"], 8545);
        assert_eq!(json["http"]["external"], "127.0.0.1:53421");
        let back: BTreeMap<String, ProcessPort> = serde_json::from_value(json).unwrap();
        assert_eq!(back, ports);
    }
}
