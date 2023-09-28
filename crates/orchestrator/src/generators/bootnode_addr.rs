use std::{fmt::Display, net::IpAddr};

use super::errors::GeneratorError;

pub fn generate<T: AsRef<str> + Display>(
    peer_id: &str,
    ip: &IpAddr,
    port: u16,
    args: &[T],
    // args: &[&String],
    p2p_cert: &Option<String>,
) -> Result<String, GeneratorError> {
    let addr = if let Some(index) = args.iter().position(|arg| arg.as_ref().eq("--listen-addr")) {
        let listen_value = args
            .as_ref()
            .get(index + 1)
            .ok_or(GeneratorError::BootnodeAddrGeneration(
                "can not generate bootnode address from args".into(),
            ))?
            .to_string();

        let ip_str = ip.to_string();
        let port_str = port.to_string();
        let mut parts = listen_value.split('/').collect::<Vec<&str>>();
        parts[2] = ip_str.as_str();
        parts[4] = port_str.as_str();
        parts.join("/")
    } else {
        format!("/ip4/{ip}/tcp/{port}/ws")
    };

    let mut addr_with_peer = format!("{addr}/p2p/{peer_id}");
    if let Some(p2p_cert) = p2p_cert {
        addr_with_peer.push_str("/certhash/");
        addr_with_peer.push_str(p2p_cert)
    }
    Ok(addr_with_peer)
}

#[cfg(test)]
mod tests {

    use provider::constants::LOCALHOST;

    use super::*;
    #[test]
    fn generate_for_alice_without_args() {
        let peer_id = "12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm"; // from alice as seed
        let args: Vec<&str> = vec![];
        let bootnode_addr = generate(peer_id, &LOCALHOST, 5678, &args, &None).unwrap();
        assert_eq!(
            &bootnode_addr,
            "/ip4/127.0.0.1/tcp/5678/ws/p2p/12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm"
        );
    }

    #[test]
    fn generate_for_alice_with_listen_addr() {
        // Should override the ip/port
        let peer_id = "12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm"; // from alice as seed
        let args: Vec<String> = [
            "--some",
            "other",
            "--listen-addr",
            "/ip4/192.168.100.1/tcp/30333/ws",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect();
        let bootnode_addr =
            generate(peer_id, &LOCALHOST, 5678, args.iter().as_ref(), &None).unwrap();
        assert_eq!(
            &bootnode_addr,
            "/ip4/127.0.0.1/tcp/5678/ws/p2p/12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm"
        );
    }

    #[test]
    fn generate_for_alice_withcert() {
        let peer_id = "12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm"; // from alice as seed
        let args: Vec<&str> = vec![];
        let bootnode_addr = generate(
            peer_id,
            &LOCALHOST,
            5678,
            &args,
            &Some(String::from("data")),
        )
        .unwrap();
        assert_eq!(
            &bootnode_addr,
            "/ip4/127.0.0.1/tcp/5678/ws/p2p/12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm/certhash/data"
        );
    }
}
