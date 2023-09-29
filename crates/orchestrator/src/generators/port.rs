use std::net::TcpListener;

use configuration::shared::types::Port;

use super::errors::GeneratorError;
use crate::shared::types::ParkedPort;

// TODO: (team), we want to continue support ws_port? No
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
    Ok(ParkedPort::new(port, listener))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generate_random() {
        let port = generate(None).unwrap();
        let listener = port.1.write().unwrap();

        assert!(listener.is_some());
    }

    #[test]
    fn generate_fixed_port() {
        let port = generate(Some(33056)).unwrap();
        let listener = port.1.write().unwrap();

        assert!(listener.is_some());
        assert_eq!(port.0, 33056);
    }
}
