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
// default command template to build chain-spec using runtime when chain is named
pub const DEFAULT_CHAIN_SPEC_TPL_USING_RUNTIME_NAMED_PRESET_OMMAND: &str = 
    "{{mainCommand}} chain-spec-builder create --runtime {{runtimePath}} named-preset {{chainName}}";
// default command template to build chain-spec using runtime when no name
pub const DEFAULT_CHAIN_SPEC_TPL_USING_RUNTIME_DEFAULT_COMMAND: &str = 
    "{{mainCommand}} chain-spec-builder create --runtime {{runtimePath}} default";
// default command template to check available presets
pub const DEFAULT_REQUEST_PRESET_TPL_COMMAND: &str = 
    "{{mainCommand}} chain-spec-builder list-presets --runtime {{runtimePath}}";