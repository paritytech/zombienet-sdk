use super::types::ParaId;

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("relaychain.{0}")]
    Relaychain(anyhow::Error),

    #[error("parachain[{0}].{1}")]
    Parachain(ParaId, anyhow::Error),

    #[error("global_settings.{0}")]
    GlobalSettings(anyhow::Error),

    #[error("node[{0}].{1}")]
    Node(String, anyhow::Error),

    #[error("collator[{0}].{1}")]
    Collator(String, anyhow::Error),

    #[error("resources.{0}")]
    Resources(anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum FieldError {
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

    #[error("request_memory: {0}")]
    RequestMemory(anyhow::Error),

    #[error("request_cpu: {0}")]
    RequestCpu(anyhow::Error),

    #[error("limit_memory: {0}")]
    LimitMemory(anyhow::Error),

    #[error("limit_cpu: {0}")]
    LimitCpu(anyhow::Error),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum ConversionError {
    #[error("'{0}' shouldn't contains whitespace")]
    ContainsWhitespaces(String),

    #[error("'{}' doesn't match regex '{}'", .value, .regex)]
    DoesntMatchRegex { value: String, regex: String },
}
