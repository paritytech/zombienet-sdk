use array_bytes::bytes2hex;
use codec::{CompactAs, Decode, Encode, MaxEncodedLen};
use support::substorage::storage_value_key;
use tracing::trace;

/// Parachain id.
///
/// This is an equivalent of the `polkadot_parachain_primitives::Id`, which is a compact-encoded
/// `u32`.
#[derive(
    Clone,
    CompactAs,
    Copy,
    Decode,
    Default,
    Encode,
    Eq,
    Hash,
    MaxEncodedLen,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub struct ParaId(pub u32);

// for CoretimeAssignmentProvider
pub fn generate_old(core_index: u32, para_id: u32) -> (String, String) {
    let para_id = ParaId(para_id);
    let para_hex = bytes2hex("", para_id.encode());
    let core_descriptor_prefix = bytes2hex(
        "0x",
        storage_value_key(&b"CoretimeAssignmentProvider"[..], b"CoreDescriptors"),
    );

    // CoretimeAssignmentProvider CoreDescriptors <idx>
    let core_descriptor_idx_key = format!(
        "{core_descriptor_prefix}{}",
        bytes2hex("", sp_core::twox_256(&core_index.to_le_bytes()))
    );
    let core_descriptor = format!("0x00010402{}00e100e100010000e1", para_hex);

    trace!("CoreDescriptor (raw) - {core_descriptor_idx_key}:{core_descriptor}");
    (core_descriptor_idx_key, core_descriptor)
}

// for ParaScheduler
pub fn get_parascheduler_storage_key() -> String {
    bytes2hex(
        "0x",
        storage_value_key(&b"ParaScheduler"[..], b"CoreDescriptors"),
    )
}

pub fn generate(core_index: u32, para_id: u32) -> String {
    let para_id = ParaId(para_id);
    let para_hex = bytes2hex("", para_id.encode());
    let core_index_hex = bytes2hex("", core_index.encode());

    let core_descriptor = format!("{core_index_hex}00010402{para_hex}00e100e100010000e1");

    trace!("CoreDescriptor part (raw): {core_descriptor}");
    core_descriptor
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn generate_old_works() {
        let (k, v) = generate_old(0, 1000);
        println!("key: {k}, val: {v}");
    }

    #[test]
    fn generate_works() {
        let part = generate(0, 1000);
        println!("part: {part}");
    }

    #[test]
    fn get_parascheduler_storage_key_works() {
        let k = get_parascheduler_storage_key();
        println!("ParaScheduler.CoreDescriptors (key): {k}");
    }
}
