#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("Generating key {0} with input {1}")]
    KeyGeneration(String, String),
    #[error("Generating port {0}, err {1}")]
    PortGeneration(u16, String),
}
