use hex::FromHex;
use libp2p::identity::{ed25519, Keypair};
use sha2::digest::Digest;

use super::errors::GeneratorError;

// Generate p2p identity for node
// return `node-key` and `peerId`
pub fn generate(node_name: &str) -> Result<(String, String), GeneratorError> {
    let key = hex::encode(sha2::Sha256::digest(node_name));

    let bytes = <[u8; 32]>::from_hex(key.clone()).map_err(|_| {
        GeneratorError::IdentityGeneration("can not transform hex to [u8;32]".into())
    })?;
    let sk = ed25519::SecretKey::try_from_bytes(bytes)
        .map_err(|_| GeneratorError::IdentityGeneration("can not create sk from bytes".into()))?;
    let local_identity: Keypair = ed25519::Keypair::from(sk).into();
    let local_public = local_identity.public();
    let local_peer_id = local_public.to_peer_id();

    Ok((key, local_peer_id.to_base58()))
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn generate_for_alice() {
        let s = "alice";
        let (key, peer_id) = generate(s).unwrap();
        assert_eq!(
            &key,
            "2bd806c97f0e00af1a1fc3328fa763a9269723c8db8fac4f93af71db186d6e90"
        );
        assert_eq!(
            &peer_id,
            "12D3KooWQCkBm1BYtkHpocxCwMgR8yjitEeHGx8spzcDLGt2gkBm"
        );
    }
}
