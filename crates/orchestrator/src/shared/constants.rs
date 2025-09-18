/// Prometheus exporter default port
pub const PROMETHEUS_PORT: u16 = 9615;
/// Prometheus exporter default port in collator full-node
pub const FULL_NODE_PROMETHEUS_PORT: u16 = 9616;
/// JSON-RPC server (ws)
pub const RPC_PORT: u16 = 9944;
// JSON-RPC server (http, used by old versions)
pub const RPC_HTTP_PORT: u16 = 9933;
// P2P default port
pub const P2P_PORT: u16 = 30333;
// default command template to build chain-spec
pub const DEFAULT_CHAIN_SPEC_TPL_COMMAND: &str =
    "{{mainCommand}} build-spec --chain {{chainName}} {{disableBootnodes}}";
// interval to determine how often to run node liveness checks
pub const NODE_MONITORING_INTERVAL_SECONDS: u64 = 30;
