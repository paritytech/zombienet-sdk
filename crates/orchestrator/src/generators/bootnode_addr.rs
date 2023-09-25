use std::net::IpAddr;

use super::errors::GeneratorError;

pub fn generate(
    peer_id: &str,
    ip: &IpAddr,
    port: u16,
    args: &Vec<String>,
    p2p_cert: &Option<String>,
) -> Result<String, GeneratorError> {
    let addr = if let Some(index) = args.iter().position(|arg| arg.eq("--lister-addr")) {
        let listen_value = args
            .get(index + 1)
            .ok_or(GeneratorError::BootnodeAddrGeneration(
                "can not generate bootnode address from args".into(),
            ))?;
        let ip_str = ip.to_string();
        let port_str = port.to_string();
        let mut parts = listen_value.split("/").collect::<Vec<&str>>();
        parts[2] = ip_str.as_str();
        parts[4] = port_str.as_str();
        let mut addr = parts.join("/");
        if let Some(p2p_cert) = p2p_cert {
            addr.push_str("/certhash/");
            addr.push_str(p2p_cert)
        }
        addr
    } else {
        format!("/ip4/{ip}/tcp/{port}/ws")
    };

    Ok(format!("{addr}/p2p/{peer_id}"))
}
