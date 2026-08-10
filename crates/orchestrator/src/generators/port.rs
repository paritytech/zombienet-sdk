use std::net::{SocketAddr, TcpListener};

use configuration::shared::types::Port;
use socket2::{Domain, Protocol, Socket, Type};
use support::constants::THIS_IS_A_BUG;

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
    let addr: SocketAddr = format!("[::]:{port}")
        .parse()
        .expect("addr should be valid");
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))
        .map_err(|_e| GeneratorError::PortGeneration(port, "Can't create the socket".into()))?;

    // Explicitly disable v6only for dual-stack support
    socket.set_only_v6(false).map_err(|_e| {
        GeneratorError::PortGeneration(port, "Can't set v6 only to false in socket".into())
    })?;
    socket
        .bind(&addr.into())
        .map_err(|_e| GeneratorError::PortGeneration(port, "Can't bind in socket".into()))?;
    socket
        .listen(128)
        .map_err(|_e| GeneratorError::PortGeneration(port, "Can't listen in socket".into()))?;

    let listener: TcpListener = socket.into();
    let port = listener
        .local_addr()
        .expect(&format!(
            "We should always get the local_addr from the listener {THIS_IS_A_BUG}"
        ))
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
