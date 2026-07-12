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
// Default command template to export a chain-spec.
// `export-chain-spec` replaced the deprecated `build-spec` CLI in polkadot-sdk.
// Bootnodes are customized after generation, so `--disable-default-bootnode` is unused.
pub const DEFAULT_CHAIN_SPEC_TPL_COMMAND: &str =
    "{{mainCommand}} export-chain-spec --chain {{chainName}}";
// interval to determine how often to run node liveness checks
pub const NODE_MONITORING_INTERVAL_SECONDS: u64 = 15;
// how long to wait before a node is considered unresponsive
pub const NODE_MONITORING_FAILURE_THRESHOLD_SECONDS: u64 = 5;
// metric used to check if the node is running
pub const PROCESS_START_TIME_METRIC: &str = "process_start_time_seconds";
