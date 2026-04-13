use sp_core::storage::StorageKey;

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
