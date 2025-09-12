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
// default maximum time in seconds to wait for a node to be up
pub const DEFAULT_NODE_SPAWN_TIMEOUT_SECONDS: u64 = 300;
// default time to wait after the node is spawned to start monitoring its liveness
pub const DEFAULT_INITIAL_NODE_MONITORING_DELAY_SECONDS: u64 = 60;
// default node monitoring interval
pub const DEFAULT_NODE_MONITORING_INTERVAL_SECONDS: u64 = 10;
// default time to wait before monitoring task considers a node failed
pub const DEFAULT_NODE_MONITORING_LIVENESS_TIMEOUT_SECONDS: u64 = 5;
