use crate::{generators::errors::GeneratorError, shared::types::{Accounts, NodeAccount}};
use jam_std_common::SecretKeyset;
use jam_std_common::hash_raw;
use jam_types::hex;


pub fn generate(seed: &str) -> Result<Accounts, GeneratorError> {
    let seed = hash_raw(seed.as_bytes());
    let mut accounts: Accounts = Default::default();
    let secrets = SecretKeyset::from_seed(&seed);
    let pubs = secrets.public(Default::default());
    accounts.insert("ed25519".into(), NodeAccount::new("", hex::to_hex(pubs.ed25519.as_bytes()).to_string()));
    accounts.insert("bandersnatch".into(), NodeAccount::new("", hex::to_hex(pubs.bandersnatch.as_ref()).to_string()));
    Ok(accounts)
}