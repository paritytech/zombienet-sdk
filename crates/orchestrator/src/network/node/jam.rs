//! JAM nodes.

use std::{net::IpAddr, time::Duration};

use anyhow::anyhow;
use async_trait::async_trait;
use configuration::types::{Arg, AssetLocation, JamNodeMode};
use serde::{Deserialize, Serialize};
use support::net::wait_tcp_ready;
use tracing::debug;

use super::{
    core::NodeCore,
    spawned::{NodeKind, SpawnedNode},
};
use crate::{
    generators::{generate_jam_node_command, GenCmdOptions},
    network_spec::jamnode::JamNodeSpec,
};

/// A running JAM node.
///
/// Same provider-generic behaviour as any other node (see [`NodeCore`]), plus
/// the JAM specifics: the libp2p peer identity used to build bootnode
/// addresses, the node mode and the rpc endpoint.
///
/// Unlike [`NetworkNode`](super::NetworkNode) it exposes no subxt client and
/// no Prometheus metrics assertions, since JAM nodes serve neither.
#[derive(Clone, Serialize)]
pub struct JamNetworkNode {
    #[serde(flatten)]
    pub(crate) core: NodeCore,
    pub(crate) spec: JamNodeSpec,
    /// Ip the node is reachable at.
    pub(crate) ip: IpAddr,
    /// `ip:rpc_port`, only bound by nodes running in `Ordinary` mode.
    pub(crate) rpc_uri: String,
    /// `{peer_id}@{ip}:{port}`, the form other nodes take as `--bootnode`.
    pub(crate) peer_addr: String,
    // Store the options used to generate the cmd, so we can recalculate
    // (cmd, args) from a modified spec on restart.
    pub(crate) cmd_generator_opts: GenCmdOptions,
}

/// Deserialization counterpart used when re-attaching to a running network.
#[derive(Deserialize)]
pub(crate) struct RawJamNetworkNode {
    pub(crate) name: String,
    pub(crate) spec: JamNodeSpec,
    pub(crate) ip: IpAddr,
    pub(crate) rpc_uri: String,
    pub(crate) peer_addr: String,
    pub(crate) cmd_generator_opts: GenCmdOptions,
    #[serde(default)]
    pub(crate) inner: serde_json::Value,
}

impl JamNetworkNode {
    pub(crate) fn new(
        name: impl Into<String>,
        inner: provider::DynNode,
        spec: JamNodeSpec,
        ip: IpAddr,
        cmd_generator_opts: GenCmdOptions,
    ) -> Self {
        let rpc_uri = format!("{ip}:{}", spec.rpc_port.0);
        let peer_addr = format!("{}@{ip}:{}", spec.peer_id, spec.port.0);

        Self {
            core: NodeCore::new(name, inner, NodeKind::Jam),
            spec,
            ip,
            rpc_uri,
            peer_addr,
            cmd_generator_opts,
        }
    }

    /// The provider-generic part of this node.
    pub fn core(&self) -> &NodeCore {
        &self.core
    }

    pub fn name(&self) -> &str {
        self.core.name()
    }

    pub fn spec(&self) -> &JamNodeSpec {
        &self.spec
    }

    /// Mode this node runs in (`ordinary`, `validator` or `proxy`).
    pub fn mode(&self) -> &JamNodeMode {
        &self.spec.mode
    }

    /// The node libp2p local identity.
    pub fn peer_id(&self) -> &str {
        &self.spec.peer_id
    }

    /// `{peer_id}@{ip}:{port}`, as passed to other nodes with `--bootnode`.
    pub fn peer_addr(&self) -> &str {
        &self.peer_addr
    }

    /// `ip:rpc_port`.
    ///
    /// NOTE: only nodes running in [`JamNodeMode::Ordinary`] are started with
    /// `--rpc-port`, so this is not bound for validators/proxies.
    pub fn rpc_uri(&self) -> &str {
        &self.rpc_uri
    }

    /// Address used to probe for readiness: the rpc port for ordinary nodes
    /// (the only ones that bind it), the p2p port otherwise.
    fn probe_addr(&self) -> String {
        match self.spec.mode {
            JamNodeMode::Ordinary => self.rpc_uri.clone(),
            JamNodeMode::Validator | JamNodeMode::Proxy => {
                format!("{}:{}", self.ip, self.spec.port.0)
            },
        }
    }

    /// Check if the node is responsive by attempting to connect to it.
    ///
    /// This performs an actual connection attempt with a short timeout (2 seconds).
    /// Returns `true` if the node is reachable and responding, `false` otherwise.
    ///
    /// This is more robust than `is_running()` as it verifies the node is actually alive.
    pub async fn is_responsive(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(2), wait_tcp_ready(&self.probe_addr()))
            .await
            .is_ok()
    }

    /// Restart the node using the optional provided:
    /// - Assets: binaries to download
    /// - cmd: program to exec.
    /// - args: Arguments to override the ones provided in config.
    ///
    /// The node will be restarted with the same `env` and isolated directory
    /// (e.g same database).
    pub async fn restart_with(
        &self,
        assets: Vec<AssetLocation>,
        program: Option<String>,
        args: Option<Vec<Arg>>,
        after: Option<Duration>,
    ) -> Result<(), anyhow::Error> {
        let mut spec_cloned = self.spec.clone();

        if let Some(args) = args {
            spec_cloned.args = args;
        }
        if let Some(program) = program {
            spec_cloned.command = program.as_str().try_into()?;
        }

        let (program, args) =
            generate_jam_node_command(&spec_cloned, self.cmd_generator_opts.clone());

        self.core
            .restart_with(&assets, &program, &args, after)
            .await
    }
}

#[async_trait]
impl SpawnedNode for JamNetworkNode {
    fn core(&self) -> &NodeCore {
        &self.core
    }

    /// JAM nodes expose no Prometheus endpoint, so readiness is established by
    /// connecting to the port the node binds for its mode.
    async fn wait_until_is_up(&self, timeout_secs: u64) -> Result<(), anyhow::Error> {
        // Validators and proxies speak QUIC over UDP on their p2p port; a TCP probe can
        // never succeed there. Only the ordinary node exposes a TCP (RPC) endpoint to wait
        // on.
        if !matches!(self.spec.mode, JamNodeMode::Ordinary) {
            debug!("[{}] validator/proxy p2p is UDP; skipping TCP readiness wait", self.name());
            return Ok(());
        }
        let addr = self.probe_addr();
        debug!("[{}] waiting until {addr} is reachable", self.name());

        tokio::time::timeout(Duration::from_secs(timeout_secs), wait_tcp_ready(&addr))
            .await
            .map_err(|_| {
                anyhow!(
                    "Timeout ({timeout_secs}), waiting for {} to be up at {addr}",
                    self.name()
                )
            })?
            .map_err(|err| anyhow!("{}: {:?}", self.name(), err))
    }
}

impl std::fmt::Debug for JamNetworkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JamNetworkNode")
            .field("inner", &"inner_skipped")
            .field("spec", &self.spec)
            .field("name", &self.name())
            .field("mode", &self.spec.mode)
            .field("peer_addr", &self.peer_addr)
            .field("rpc_uri", &self.rpc_uri)
            .finish()
    }
}
