use array_bytes::bytes2hex;
use codec::{CompactAs, Decode, Encode, MaxEncodedLen};
use sp_core::storage::StorageKey;
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

    trace!("{core_descriptor_idx_key}:{core_descriptor}");
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

    trace!("core_descriptor part: {core_descriptor}");
    core_descriptor
}

// Helpers from subhasher / substorage

/// Calculate the storage key of a pallet `StorageValue` item.
pub fn storage_value_key<A, B>(pallet: A, item: B) -> StorageKey
where
    A: AsRef<[u8]>,
    B: AsRef<[u8]>,
{
    let mut k = Vec::new();

    k.extend_from_slice(&sp_core::twox_128(pallet.as_ref()));
    k.extend_from_slice(&sp_core::twox_128(item.as_ref()));

    StorageKey(k)
}
