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
    InvalidChain(E),

    #[error("image: {0}")]
    InvalidImage(E),

    #[error("default_image: {0}")]
    InvalidDefaultImage(E),

    #[error("command: {0}")]
    InvalidCommand(E),

    #[error("default_command: {0}")]
    InvalidDefaultCommand(E),

    #[error("db_snapshot: {0}")]
    InvalidDbSnapshot(E),

    #[error("default_db_snapshot: {0}")]
    InvalidDefaultDbSnapshot(E),

    #[error("bootnodes_addresses[{0}]: {1}")]
    InvalidBootnodesAddress(usize, E),

    #[error("chain_spec_path: {0}")]
    InvalidChainSpecPath(E),

    #[error("genesis_wasm_path: {0}")]
    InvalidGenesisWasmPath(E),

    #[error("genesis_wasm_generator: {0}")]
    InvalidGenesisWasmGenerator(E),

    #[error("genesis_state_path: {0}")]
    InvalidGenesisStatePath(E),

    #[error("genesis_state_generator: {0}")]
    InvalidGenesisStateGenerator(E),

    #[error("local_ip: {0}")]
    InvalidLocalIp(E),

    #[error("request_memory: {0}")]
    InvalidRequestMemory(E),

    #[error("request_cpu: {0}")]
    InvalidRequestCpu(E),

    #[error("limit_memory: {0}")]
    InvalidLimitMemory(E),

    #[error("limit_cpu: {0}")]
    InvalidLimitCpu(E),
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
