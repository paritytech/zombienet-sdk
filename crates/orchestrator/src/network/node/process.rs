//! Custom processes: the services a network config declares under
//! `[[custom_processes]]`, running next to the nodes.
//!
//! A [`CustomProcessNode`] is a member of the network like any other node:
//! it is tracked on [`Network`](crate::network::Network), written to
//! `zombie.json`, recreated on attach and destroyed with the namespace. It
//! has no chain, so the only protocol-level surface it exposes is the set of
//! ports it declared and where each one is reachable.

use std::{collections::BTreeMap, fmt, net::IpAddr, time::Duration};

use async_trait::async_trait;
use configuration::{types::Port, CustomProcess};
use provider::types::ProcessStatus;
use serde::{Deserialize, Serialize};
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

/// Where a custom process is in its lifecycle. In `zombie.json` it is the
/// `state` field (`starting`, `ready`, `completed`, `failed`) plus
/// `state_reason` for a failure; a record without one is taken as ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum CustomProcessState {
    /// Spawned, its condition not met yet.
    Starting,
    /// Up: passed its ready check, or had nothing to check.
    Ready,
    /// A one-shot that exited 0.
    Completed,
    /// The ready check failed or timed out, or the one-shot exited with an
    /// error. The process is left as it is, for its logs.
    Failed {
        #[serde(rename = "state_reason")]
        reason: String,
    },
}

impl fmt::Display for CustomProcessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Completed => "completed",
            Self::Failed { .. } => "failed",
        })
    }
}

/// A custom process that was never spawned: a dependency did not meet its
/// condition, or its ports could not be reserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedProcess {
    pub name: String,
    pub reason: String,
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
    #[serde(flatten)]
    pub(crate) state: CustomProcessState,
}

/// Deserialization counterpart used when re-attaching to a running network.
#[derive(Deserialize)]
pub(crate) struct RawCustomProcessNode {
    pub(crate) name: String,
    pub(crate) spec: CustomProcess,
    pub(crate) ip: IpAddr,
    #[serde(default)]
    pub(crate) ports: BTreeMap<String, ProcessPort>,
    /// `None` for a record from before states were kept, or one this version
    /// cannot read (a state it does not know); taken as up either way.
    #[serde(flatten)]
    pub(crate) state: Option<CustomProcessState>,
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
            state,
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

    /// Where the process is in its lifecycle, as of the spawn (or the record
    /// attached to); [`Self::wait_ready`] re-checks.
    pub fn state(&self) -> &CustomProcessState {
        &self.state
    }

    /// The first declared port, in declaration order, if any.
    fn first_port(&self) -> Option<&ProcessPort> {
        self.spec
            .ports()
            .first()
            .and_then(|declared| self.ports.get(&declared.name))
    }

    /// Wait for the process to meet its condition and say what happened: a
    /// one-shot to exit (0 is [`CustomProcessState::Completed`]), any other
    /// to pass its ready check (the provider's own, where it has one, else a
    /// probe from here). A one-shot that already finished is reported as
    /// recorded: there is nothing left to observe, and after an attach its
    /// pid is gone along with its exit code.
    pub async fn wait_ready(&self, timeout: Duration) -> CustomProcessState {
        if self.spec.one_shot()
            && matches!(
                self.state,
                CustomProcessState::Completed | CustomProcessState::Failed { .. }
            )
        {
            return self.state.clone();
        }
        match self.await_condition(timeout).await {
            Ok(state) => state,
            Err(reason) => CustomProcessState::Failed { reason },
        }
    }

    async fn await_condition(&self, timeout: Duration) -> Result<CustomProcessState, String> {
        let interval = Duration::from_secs(u64::from(
            self.spec.ready_check().map(|c| c.interval).unwrap_or(1),
        ));
        let check = self.spec.effective_check();
        let target = check
            .as_ref()
            .and_then(|check| Some((self.ports.get(check.port_name())?, check.http_path())));
        if !self.spec.one_shot() && target.is_none() {
            return Ok(CustomProcessState::Ready);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| format!("could not build the http probe: {e}"))?;

        // The last thing that stood in the way, for the timeout message. A
        // status read that fails is retried like any other miss: one API
        // hiccup must not fail the process for good.
        let mut problem = String::from("no status read yet");
        let waited = tokio::time::timeout(timeout, async {
            loop {
                let status = match self.core.inner().status().await {
                    Ok(status) => status,
                    Err(err) => {
                        problem = format!("could not read the process status: {err}");
                        tokio::time::sleep(interval).await;
                        continue;
                    },
                };
                if self.spec.one_shot() {
                    match status {
                        ProcessStatus::Exited { code: Some(0) } => {
                            return Ok(CustomProcessState::Completed)
                        },
                        ProcessStatus::Exited { code: Some(code) } => {
                            return Err(format!("exited with code {code}"))
                        },
                        ProcessStatus::Exited { code: None } => {
                            return Err("exited, with its code unknown".into())
                        },
                        ProcessStatus::Running { .. } => problem = "still running".into(),
                    }
                } else {
                    let (port, path) = target.expect("checked above; qed");
                    let kind = if path.is_some() { "http" } else { "tcp" };
                    match status {
                        ProcessStatus::Exited { code } => {
                            return Err(match code {
                                Some(code) => format!("exited with code {code}"),
                                None => "exited".into(),
                            })
                        },
                        // the provider checks (k8s: the pod's probe)
                        ProcessStatus::Running { ready: Some(true) } => {
                            return Ok(CustomProcessState::Ready)
                        },
                        ProcessStatus::Running { ready: Some(false) } => {
                            problem = format!(
                                "the provider's {kind} probe on port {}{} did not pass",
                                port.port,
                                path.unwrap_or("")
                            );
                        },
                        // nobody else checks: probe from here
                        ProcessStatus::Running { ready: None } => {
                            let passed = match path {
                                None => tcp_answers(&port.external).await,
                                Some(path) => http_answers(&client, &port.external, path).await,
                            };
                            if passed {
                                return Ok(CustomProcessState::Ready);
                            }
                            problem = format!(
                                "{kind} {}{} did not pass",
                                port.external,
                                path.unwrap_or("")
                            );
                        },
                    }
                }
                tokio::time::sleep(interval).await;
            }
        })
        .await;

        match waited {
            Ok(outcome) => outcome,
            Err(_elapsed) => Err(format!(
                "{} after {}s ({problem})",
                if self.spec.one_shot() {
                    "not finished"
                } else {
                    "not ready"
                },
                timeout.as_secs()
            )),
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
            Some(port) => tcp_answers(&port.external).await,
            None => true,
        }
    }
}

/// One TCP connect to `addr`, given 2 seconds.
async fn tcp_answers(addr: &str) -> bool {
    tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(addr))
        .await
        .is_ok_and(|connected| connected.is_ok())
}

/// One `GET http://<addr><path>`, given 5 seconds, passing on 2xx.
async fn http_answers(client: &reqwest::Client, addr: &str, path: &str) -> bool {
    client
        .get(format!("http://{addr}{path}"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .is_ok_and(|res| res.status().is_success())
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
            "[{}] custom process, not waited on; state {}",
            self.name(),
            self.state
        );
        Ok(())
    }
}

impl fmt::Debug for CustomProcessNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomProcessNode")
            .field("inner", &"inner_skipped")
            .field("name", &self.name())
            .field("spec", &self.spec)
            .field("ip", &self.ip)
            .field("ports", &self.ports)
            .field("state", &self.state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use configuration::{CustomProcessBuilder, ReadyCheck};

    use super::*;
    use crate::network::node::mock::MockNode;

    fn node(spec: CustomProcess, mock: MockNode, state: CustomProcessState) -> CustomProcessNode {
        let ports = spec
            .ports()
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    ProcessPort {
                        port: 8080,
                        external: "127.0.0.1:1".into(),
                        internal: "x:8080".into(),
                    },
                )
            })
            .collect();
        CustomProcessNode::new(
            "p",
            Arc::new(mock),
            spec,
            "127.0.0.1".parse().unwrap(),
            ports,
            state,
        )
    }

    fn one_shot() -> CustomProcess {
        CustomProcessBuilder::new()
            .with_name("p")
            .with_command("sh")
            .with_one_shot()
            .build()
            .unwrap()
    }

    fn with_check() -> CustomProcess {
        CustomProcessBuilder::new()
            .with_name("p")
            .with_command("srv")
            .with_named_port("http", 8080)
            .with_ready_check(ReadyCheck::tcp("http"))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn wait_ready_follows_what_the_provider_reports() {
        let running = ProcessStatus::Running { ready: None };
        let secs = Duration::from_secs;

        // a one-shot: running, then exit 0
        let n = node(
            one_shot(),
            MockNode::new().with_statuses(vec![
                Ok(running.clone()),
                Ok(ProcessStatus::Exited { code: Some(0) }),
            ]),
            CustomProcessState::Starting,
        );
        assert_eq!(n.wait_ready(secs(10)).await, CustomProcessState::Completed);

        // a one-shot that fails
        let n = node(
            one_shot(),
            MockNode::new().with_statuses(vec![Ok(ProcessStatus::Exited { code: Some(3) })]),
            CustomProcessState::Starting,
        );
        assert_eq!(
            n.wait_ready(secs(10)).await,
            CustomProcessState::Failed {
                reason: "exited with code 3".into()
            }
        );

        // a finished one-shot is reported as recorded, nothing is read
        let n = node(
            one_shot(),
            MockNode::new().with_statuses(vec![Err("not asked")]),
            CustomProcessState::Completed,
        );
        assert_eq!(n.wait_ready(secs(10)).await, CustomProcessState::Completed);

        // a status error is retried; the provider's probe then passes
        let n = node(
            with_check(),
            MockNode::new().with_statuses(vec![
                Err("api down"),
                Ok(ProcessStatus::Running { ready: Some(false) }),
                Ok(ProcessStatus::Running { ready: Some(true) }),
            ]),
            CustomProcessState::Starting,
        );
        assert_eq!(n.wait_ready(secs(10)).await, CustomProcessState::Ready);

        // never ready: the timeout names the last problem
        let n = node(
            with_check(),
            MockNode::new().with_statuses(vec![Ok(ProcessStatus::Running { ready: Some(false) })]),
            CustomProcessState::Starting,
        );
        assert_eq!(
            n.wait_ready(secs(2)).await,
            CustomProcessState::Failed {
                reason: "not ready after 2s (the provider's tcp probe on port 8080 did not pass)"
                    .into()
            }
        );

        // exits before becoming ready
        let n = node(
            with_check(),
            MockNode::new().with_statuses(vec![Ok(ProcessStatus::Exited { code: Some(9) })]),
            CustomProcessState::Starting,
        );
        assert_eq!(
            n.wait_ready(secs(10)).await,
            CustomProcessState::Failed {
                reason: "exited with code 9".into()
            }
        );
    }

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
        assert_eq!(
            raw.state,
            Some(CustomProcessState::Failed { reason: "x".into() })
        );

        let without_state = serde_json::json!({
            "name": "web", "spec": {"name": "web", "command": "web"}, "ip": "10.0.0.1", "inner": {}
        });
        let raw: RawCustomProcessNode = serde_json::from_value(without_state).unwrap();
        assert_eq!(raw.state, None);
    }

    #[test]
    fn a_raw_process_without_ports_still_deserializes_but_needs_its_inner() {
        let json = serde_json::json!({
            "name": "eth-rpc",
            "spec": { "name": "eth-rpc", "command": "eth-rpc" },
            "ip": "10.0.0.1",
            "inner": { "name": "eth-rpc" }
        });
        let raw: RawCustomProcessNode = serde_json::from_value(json).unwrap();
        assert_eq!(raw.name, "eth-rpc");
        assert!(raw.ports.is_empty());

        let json = serde_json::json!({
            "name": "eth-rpc",
            "spec": { "name": "eth-rpc", "command": "eth-rpc" },
            "ip": "10.0.0.1"
        });
        assert!(serde_json::from_value::<RawCustomProcessNode>(json).is_err());
    }

    #[test]
    fn ports_round_trip_by_name() {
        let port = ProcessPort {
            port: 8545,
            external: "127.0.0.1:53421".into(),
            internal: "eth-rpc:8545".into(),
        };
        let json = serde_json::to_value(&port).unwrap();
        assert_eq!(json["external"], "127.0.0.1:53421");
        let back: ProcessPort = serde_json::from_value(json).unwrap();
        assert_eq!(back, port);
    }
}
