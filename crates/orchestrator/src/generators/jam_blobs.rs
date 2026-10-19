//! The PVM blobs zombienet embeds to build a JAM genesis without any external file.

use jam_std_common::hash_raw;
use jam_types::{AuthConfig, Authorizer, AuthorizerHash};

// With the `jam` feature, the bootstrap service (`BOOTSTRAP_SERVICE_BLOB`,
// `BOOTSTRAP_SERVICE_HASH` and `BOOTSTRAP_SERVICE_SOURCE`), built by `build.rs` from the
// `jam-bootstrap-service` crate. It is installed as service 0 unless the genesis names another
// blob.
include!(concat!(env!("OUT_DIR"), "/jam_blobs.rs"));

/// The embedded bootstrap service and where it comes from, if zombienet was built with the `jam`
/// feature.
pub fn embedded_bootstrap_service() -> Option<(&'static [u8], &'static str)> {
    #[cfg(feature = "jam")]
    return Some((BOOTSTRAP_SERVICE_BLOB, BOOTSTRAP_SERVICE_SOURCE));
    #[cfg(not(feature = "jam"))]
    None
}

/// The null authorizer, which always authorizes.
pub const NULL_AUTHORIZER_BLOB: &[u8] = jam_null_authorizer_bin::BLOB;

/// The code hash of [`NULL_AUTHORIZER_BLOB`].
pub const NULL_AUTHORIZER_HASH: [u8; 32] = jam_null_authorizer_bin::HASH;

/// The hash a work package names to be authorized by the parameterless authorizer `code`.
pub fn authorizer_hash(code: &[u8], config: &[u8]) -> AuthorizerHash {
    Authorizer {
        code_hash: hash_raw(code).into(),
        config: AuthConfig(config.to_vec()),
    }
    .hash(hash_raw)
}

/// The hash a work package names to be authorized by the null authorizer.
pub fn null_authorizer_hash() -> AuthorizerHash {
    authorizer_hash(NULL_AUTHORIZER_BLOB, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_hashes_match_the_blobs() {
        #[cfg(feature = "jam")]
        assert_eq!(hash_raw(BOOTSTRAP_SERVICE_BLOB), BOOTSTRAP_SERVICE_HASH);
        assert_eq!(hash_raw(NULL_AUTHORIZER_BLOB), NULL_AUTHORIZER_HASH);
    }
}
