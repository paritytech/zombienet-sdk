//! Custom processes: the services a network config declares under
//! `[[custom_processes]]`, running next to the nodes.
//!
//! A [`CustomProcessNode`] is a member of the network like any other node:
//! it is tracked on [`Network`](crate::network::Network), written to
//! `zombie.json`, recreated on attach and destroyed with the namespace. It
//! has no chain, so the only protocol-level surface it exposes is the set of
//! ports it declared and where each one is reachable.

use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::{types::Port, CustomProcess};
use provider::types::ProcessStatus;
use serde::{Deserialize, Serialize, Serializer};
use support::net::wait_tcp_ready;
use tracing::{debug, info, warn};

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

/// Where a custom process is in its lifecycle. In `zombie.json` it is the
/// `state` field (`starting`, `ready`, `completed`, `failed`) plus
/// `state_reason` for a failure; a record without one is taken as ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StateRepr", into = "StateRepr")]
pub enum CustomProcessState {
    /// Spawned, its condition not met yet.
    Starting,
    /// Up: passed its ready check, or had nothing to check.
    Ready,
    /// A one-shot that exited 0.
    Completed,
    /// The ready check failed or timed out, or the one-shot exited with an
    /// error. The process is left as it is, for its logs.
    Failed { reason: String },
}

#[derive(Serialize, Deserialize)]
struct StateRepr {
    #[serde(default = "ready_str")]
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state_reason: Option<String>,
}

fn ready_str() -> String {
    "ready".into()
}

impl From<StateRepr> for CustomProcessState {
    fn from(repr: StateRepr) -> Self {
        match repr.state.as_str() {
            "starting" => Self::Starting,
            "completed" => Self::Completed,
            "failed" => Self::Failed {
                reason: repr.state_reason.unwrap_or_default(),
            },
            _ => Self::Ready,
        }
    }
}

impl From<CustomProcessState> for StateRepr {
    fn from(state: CustomProcessState) -> Self {
        let (state, state_reason) = match state {
            CustomProcessState::Starting => ("starting", None),
            CustomProcessState::Ready => ("ready", None),
            CustomProcessState::Completed => ("completed", None),
            CustomProcessState::Failed { reason } => ("failed", Some(reason)),
        };
        Self {
            state: state.into(),
            state_reason,
        }
    }
}

/// A custom process that was never spawned: a dependency did not meet its
/// condition, or its ports could not be reserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedProcess {
    pub name: String,
    pub reason: String,
}

fn serialize_state<S: Serializer>(
    state: &Arc<RwLock<CustomProcessState>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    state
        .read()
        .map_err(|_| serde::ser::Error::custom("state lock poisoned"))?
        .serialize(serializer)
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
    #[serde(flatten, serialize_with = "serialize_state")]
    pub(crate) state: Arc<RwLock<CustomProcessState>>,
}

/// Deserialization counterpart used when re-attaching to a running network.
#[derive(Deserialize)]
pub(crate) struct RawCustomProcessNode {
    pub(crate) name: String,
    pub(crate) spec: CustomProcess,
    pub(crate) ip: IpAddr,
    #[serde(default)]
    pub(crate) ports: BTreeMap<String, ProcessPort>,
    /// A record from before states were kept is taken as up.
    #[serde(flatten)]
    pub(crate) state: CustomProcessState,
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
        state: CustomProcessState,
    ) -> Self {
        Self {
            core: NodeCore::new(name, inner, NodeKind::CustomProcess),
            spec,
            ip,
            ports,
            state: Arc::new(RwLock::new(state)),
        }
    }

    /// Where the process is in its lifecycle.
    pub fn state(&self) -> CustomProcessState {
        self.state
            .read()
            .map(|s| s.clone())
            .unwrap_or(CustomProcessState::Failed {
                reason: "state lock poisoned".into(),
            })
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

    pub(crate) fn set_state(&self, state: CustomProcessState) {
        if let Ok(mut guard) = self.state.write() {
            *guard = state;
        }
    }

    /// The first declared port, in declaration order, if any.
    fn first_port(&self) -> Option<&ProcessPort> {
        self.spec
            .ports()
            .first()
            .and_then(|declared| self.ports.get(&declared.name))
    }

    /// What there is to wait on, see [`CustomProcess::effective_check`], with
    /// the port resolved to where it is reachable. `None` counts as ready at
    /// start.
    fn check_target(&self) -> Option<(&ProcessPort, Option<&str>)> {
        let check = self.spec.effective_check()?;
        let port = self.ports.get(check.port)?;
        Some((port, check.http_path))
    }

    /// Wait for the process to meet its condition: a one-shot to exit 0, any
    /// other to pass its ready check (the provider's own, where it has one,
    /// else a probe from here). The state is set to what happened, and an
    /// error says why it did not get there.
    pub async fn wait_ready(&self, timeout: Duration) -> Result<(), anyhow::Error> {
        let before = self.state();
        // A one-shot that finished stays as recorded: there is nothing left to
        // observe, and after an attach its pid is gone along with its exit code.
        if self.spec.one_shot() {
            match &before {
                CustomProcessState::Completed => return Ok(()),
                CustomProcessState::Failed { reason } => {
                    return Err(anyhow!("{}: {reason}", self.name()))
                },
                _ => {},
            }
        }
        let outcome = self.await_condition(timeout).await;
        match outcome {
            Ok(state) => {
                if state != before {
                    info!("✅ {}: {}", self.name(), describe(&state));
                }
                self.set_state(state);
                Ok(())
            },
            Err(reason) => {
                let state = CustomProcessState::Failed {
                    reason: reason.clone(),
                };
                if state != before {
                    warn!("⚠️  {}: {reason}", self.name());
                }
                self.set_state(state);
                Err(anyhow!("{}: {reason}", self.name()))
            },
        }
    }

    async fn await_condition(&self, timeout: Duration) -> Result<CustomProcessState, String> {
        let interval = Duration::from_secs(
            self.spec
                .ready_check()
                .and_then(|c| c.interval)
                .unwrap_or(1)
                .max(1),
        );
        let deadline = Instant::now().checked_add(timeout).unwrap_or_else(|| {
            Instant::now() + Duration::from_secs(configuration::MAX_TIMEOUT_SECS)
        });
        let target = self.check_target();
        let kind = match target {
            Some((_, Some(_))) => "http",
            _ => "tcp",
        };
        if !self.spec.one_shot() && target.is_none() {
            return Ok(CustomProcessState::Ready);
        }

        // A status read that fails is retried until the deadline: one API
        // hiccup must not fail the process for good.
        loop {
            let status = self.core.inner().status().await;
            let problem = match (self.spec.one_shot(), status, target) {
                (_, Err(err), _) => format!("could not read the process status: {err}"),
                (true, Ok(ProcessStatus::Exited { code: Some(0) }), _) => {
                    return Ok(CustomProcessState::Completed)
                },
                (true, Ok(ProcessStatus::Exited { code: Some(code) }), _) => {
                    return Err(format!("exited with code {code}"))
                },
                (true, Ok(ProcessStatus::Exited { code: None }), _) => {
                    return Err("exited, with its code unknown".into())
                },
                (true, Ok(ProcessStatus::Running { .. }), _) => "still running".into(),
                (false, Ok(ProcessStatus::Exited { code }), _) => {
                    return Err(match code {
                        Some(code) => format!("exited with code {code} before becoming ready"),
                        None => "exited before becoming ready".into(),
                    })
                },
                // the provider checks (k8s: the pod's probe)
                (false, Ok(ProcessStatus::Running { ready: Some(true) }), _) => {
                    return Ok(CustomProcessState::Ready)
                },
                (false, Ok(ProcessStatus::Running { ready: Some(false) }), Some((port, path))) => {
                    format!(
                        "the provider's {kind} probe on port {}{} did not pass",
                        port.port,
                        path.unwrap_or("")
                    )
                },
                // nobody else checks: probe from here
                (false, Ok(ProcessStatus::Running { ready: None }), Some((port, path))) => {
                    if probe(&port.external, path).await {
                        return Ok(CustomProcessState::Ready);
                    }
                    format!(
                        "{kind} {}{} did not pass",
                        port.external,
                        path.unwrap_or("")
                    )
                },
                (false, Ok(ProcessStatus::Running { .. }), None) => {
                    return Ok(CustomProcessState::Ready)
                },
            };
            if Instant::now() >= deadline {
                let what = if self.spec.one_shot() {
                    "not finished"
                } else {
                    "not ready"
                };
                return Err(format!("{what} after {}s ({problem})", timeout.as_secs()));
            }
            // never past the deadline, whatever the interval
            tokio::time::sleep(interval.min(deadline.saturating_duration_since(Instant::now())))
                .await;
        }
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

    /// Nothing to wait on here: the spawn already waited for the process's
    /// condition, and a failed one must not fail a network-wide wait (the
    /// network is kept up for its logs). [`Self::wait_ready`] re-checks on
    /// request.
    async fn wait_until_is_up(&self, _timeout_secs: u64) -> Result<(), anyhow::Error> {
        debug!(
            "[{}] custom process, not waited on; state {:?}",
            self.name(),
            self.state()
        );
        Ok(())
    }
}

fn describe(state: &CustomProcessState) -> &'static str {
    match state {
        CustomProcessState::Starting => "starting",
        CustomProcessState::Ready => "ready",
        CustomProcessState::Completed => "completed",
        CustomProcessState::Failed { .. } => "failed",
    }
}

/// One attempt from where zombienet runs: a TCP connect, or `GET` of `path`
/// answering 2xx. On docker a published port answers for the proxy, so
/// tcp proves less there; see the docs.
async fn probe(addr: &str, path: Option<&str>) -> bool {
    match path {
        None => tokio::time::timeout(Duration::from_secs(2), wait_tcp_ready(addr))
            .await
            .is_ok(),
        Some(path) => {
            let url = format!("http://{addr}{path}");
            let client = match reqwest::Client::builder().no_proxy().build() {
                Ok(client) => client,
                Err(_) => return false,
            };
            match client
                .get(&url)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(res) => res.status().is_success(),
                Err(_) => false,
            }
        },
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
            .field("state", &self.state())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_is_two_plain_fields_and_an_old_record_is_ready() {
        for (state, json) in [
            (CustomProcessState::Starting, r#"{"state":"starting"}"#),
            (CustomProcessState::Ready, r#"{"state":"ready"}"#),
            (CustomProcessState::Completed, r#"{"state":"completed"}"#),
            (
                CustomProcessState::Failed {
                    reason: "exited with code 3".into(),
                },
                r#"{"state":"failed","state_reason":"exited with code 3"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<CustomProcessState>(json).unwrap(),
                state
            );
        }

        let with_state = serde_json::json!({
            "name": "web", "spec": {"name": "web", "command": "web"}, "ip": "10.0.0.1",
            "state": "failed", "state_reason": "x", "inner": {}
        });
        let raw: RawCustomProcessNode = serde_json::from_value(with_state).unwrap();
        assert_eq!(raw.state, CustomProcessState::Failed { reason: "x".into() });

        let without_state = serde_json::json!({
            "name": "web", "spec": {"name": "web", "command": "web"}, "ip": "10.0.0.1", "inner": {}
        });
        let raw: RawCustomProcessNode = serde_json::from_value(without_state).unwrap();
        assert_eq!(raw.state, CustomProcessState::Ready);
    }

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
