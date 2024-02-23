use super::types::{ParaId, Port};

/// An error at the configuration level.
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("relaychain.{0}")]
    Relaychain(anyhow::Error),

    #[error("parachain[{0}].{1}")]
    Parachain(ParaId, anyhow::Error),

    #[error("global_settings.{0}")]
    GlobalSettings(anyhow::Error),

    #[error("nodes['{0}'].{1}")]
    Node(String, anyhow::Error),

    #[error("collators['{0}'].{1}")]
    Collator(String, anyhow::Error),
}

/// An error at the field level.
#[derive(thiserror::Error, Debug)]
pub enum FieldError {
    #[error("name: {0}")]
    Name(anyhow::Error),

    #[error("chain: {0}")]
    Chain(anyhow::Error),

    #[error("image: {0}")]
    Image(anyhow::Error),

    #[error("default_image: {0}")]
    DefaultImage(anyhow::Error),

    #[error("command: {0}")]
    Command(anyhow::Error),

    #[error("default_command: {0}")]
    DefaultCommand(anyhow::Error),

    #[error("bootnodes_addresses[{0}]: '{1}' {2}")]
    BootnodesAddress(usize, String, anyhow::Error),

    #[error("genesis_wasm_generator: {0}")]
    GenesisWasmGenerator(anyhow::Error),

    #[error("genesis_state_generator: {0}")]
    GenesisStateGenerator(anyhow::Error),

    #[error("local_ip: {0}")]
    LocalIp(anyhow::Error),

    #[error("default_resources.{0}")]
    DefaultResources(anyhow::Error),

    #[error("resources.{0}")]
    Resources(anyhow::Error),

    #[error("request_memory: {0}")]
    RequestMemory(anyhow::Error),

    #[error("request_cpu: {0}")]
    RequestCpu(anyhow::Error),

    #[error("limit_memory: {0}")]
    LimitMemory(anyhow::Error),

    #[error("limit_cpu: {0}")]
    LimitCpu(anyhow::Error),

    #[error("ws_port: {0}")]
    WsPort(anyhow::Error),

    #[error("rpc_port: {0}")]
    RpcPort(anyhow::Error),

    #[error("prometheus_port: {0}")]
    PrometheusPort(anyhow::Error),

    #[error("p2p_port: {0}")]
    P2pPort(anyhow::Error),

    #[error("registration_strategy: {0}")]
    RegistrationStrategy(anyhow::Error),

}

/// A conversion error for shared types across fields.
#[derive(thiserror::Error, Debug, Clone)]
pub enum ConversionError {
    #[error("'{0}' shouldn't contains whitespace")]
    ContainsWhitespaces(String),

    #[error("'{}' doesn't match regex '{}'", .value, .regex)]
    DoesntMatchRegex { value: String, regex: String },

    #[error("can't be empty")]
    CantBeEmpty,
}

/// A validation error for shared types across fields.
#[derive(thiserror::Error, Debug, Clone)]
pub enum ValidationError {
    #[error("'{0}' is already used across config")]
    PortAlreadyUsed(Port),

    #[error("'{0}' is already used across config")]
    NodeNameAlreadyUsed(String),
}
