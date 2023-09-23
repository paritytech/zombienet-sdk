use std::net::TcpListener;

use configuration::shared::types::Port;

use super::errors::GeneratorError;
use crate::shared::types::ParkedPort;

// TODO: (team), we want to continue support ws_port?
enum PortTypes {
    Rpc,
    P2P,
    Prometheus,
}

pub fn generate(port: Option<Port>) -> Result<ParkedPort, GeneratorError> {
    let port = port.unwrap_or(0);
    let listener = TcpListener::bind(format!("0.0.0.0:{port}"))
        .map_err(|_e| GeneratorError::PortGeneration(port, "Can't bind".into()))?;
    let port = listener
        .local_addr()
        .expect("We should always get the local_addr from the listener, please report as bug")
        .port();
    Ok(ParkedPort::new(port,listener))
}
