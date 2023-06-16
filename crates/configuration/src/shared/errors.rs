use super::types::ParaId;

#[derive(thiserror::Error, Debug)]
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

#[derive(thiserror::Error, Debug)]
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

    #[error("db_snapshot: {0}")]
    DbSnapshot(E),

    #[error("default_db_snapshot: {0}")]
    DefaultDbSnapshot(E),

    #[error("bootnodes_addresses[{0}]: {1}")]
    BootnodesAddress(usize, E),

    #[error("chain_spec_path: {0}")]
    ChainSpecPath(E),

    #[error("genesis_wasm_path: {0}")]
    GenesisWasmPath(E),

    #[error("genesis_wasm_generator: {0}")]
    GenesisWasmGenerator(E),

    #[error("genesis_state_path: {0}")]
    GenesisStatePath(E),

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

#[derive(thiserror::Error, Debug)]
pub enum ConversionError {
    #[error("'{0}' shouldn't contains whitespace")]
    ContainsWhitespaces(String),

    #[error("'{}' doesn't match regex '{}'", .value, .regex)]
    DoesntMatchRegex { value: String, regex: String },

    #[error("unable to convert '{0}' into url::Url or path::PathBuf")]
    InvalidUrlOrPathBuf(String),
}
