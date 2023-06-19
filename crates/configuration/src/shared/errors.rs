use super::types::ParaId;

#[derive(thiserror::Error, Debug, Clone)]
pub enum ConfigError<E> {
    #[error("relaychain.{0}")]
    Relaychain(E),

    #[error("parachain[{0}].{1}")]
    Parachain(ParaId, E),

    #[error("global_settings.{0}")]
    GlobalSettings(E),

    #[error("node[{0}].{1}")]
    Node(String, E),

    #[error("collator[{0}].{1}")]
    Collator(String, E),

    #[error("resources.{0}")]
    Resources(E),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum FieldError<E> {
    #[error("chain: {0}")]
    Chain(E),

    #[error("image: {0}")]
    Image(E),

    #[error("default_image: {0}")]
    DefaultImage(E),

    #[error("command: {0}")]
    Command(E),

    #[error("default_command: {0}")]
    DefaultCommand(E),

    #[error("bootnodes_addresses[{0}]: '{1}' {2}")]
    BootnodesAddress(usize, String, E),

    #[error("genesis_wasm_generator: {0}")]
    GenesisWasmGenerator(E),

    #[error("genesis_state_generator: {0}")]
    GenesisStateGenerator(E),

    #[error("local_ip: {0}")]
    LocalIp(E),

    #[error("request_memory: {0}")]
    RequestMemory(E),

    #[error("request_cpu: {0}")]
    RequestCpu(E),

    #[error("limit_memory: {0}")]
    LimitMemory(E),

    #[error("limit_cpu: {0}")]
    LimitCpu(E),
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum ConversionError {
    #[error("'{0}' shouldn't contains whitespace")]
    ContainsWhitespaces(String),

    #[error("'{}' doesn't match regex '{}'", .value, .regex)]
    DoesntMatchRegex { value: String, regex: String },
}
