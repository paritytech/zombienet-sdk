use std::{
    path::{Path, PathBuf},
    vec,
};

use hex::encode;
use support::{constants::THIS_IS_A_BUG, fs::FileSystem};

use super::errors::GeneratorError;
use crate::{shared::types::NodeAccounts, ScopedFilesystem};

const PREFIXES: [&str; 11] = [
    "aura", "babe", "imon", "gran", "audi", "asgn", "para", "beef", // Beffy
    "nmbs", // Nimbus
    "rand", // Randomness (Moonbeam)
    "rate", // Equilibrium rate module
];

pub async fn generate<'a, T>(
    acc: &NodeAccounts,
    node_files_path: impl AsRef<Path>,
    scoped_fs: &ScopedFilesystem<'a, T>,
    asset_hub_polkadot: bool,
) -> Result<Vec<PathBuf>, GeneratorError>
where
    T: FileSystem,
{
    // Create local keystore
    scoped_fs.create_dir_all(node_files_path.as_ref()).await?;
    let mut filenames = vec![];

    let f = PREFIXES.map(|k| {
        // let filename = encode(k);

        let filename = match k {
            "aura" if asset_hub_polkadot => {
                let pk = acc
                    .accounts
                    .get("ed")
                    .expect(&format!("Key 'ed' should be set for node {THIS_IS_A_BUG}"))
                    .public_key
                    .as_str();
                format!("{}{}", encode(k), pk)
            },
            "babe" | "imon" | "audi" | "asgn" | "para" | "nmbs" | "rand" | "aura" => {
                let pk = acc
                    .accounts
                    .get("sr")
                    .expect(&format!("Key 'sr' should be set for node {THIS_IS_A_BUG}"))
                    .public_key
                    .as_str();
                format!("{}{}", encode(k), pk)
            },
            "gran" | "rate" => {
                let pk = acc
                    .accounts
                    .get("ed")
                    .expect(&format!("Key 'ed' should be set for node {THIS_IS_A_BUG}"))
                    .public_key
                    .as_str();
                format!("{}{}", encode(k), pk)
            },
            "beef" => {
                let pk = acc
                    .accounts
                    .get("ec")
                    .expect(&format!("Key 'ec' should be set for node {THIS_IS_A_BUG}"))
                    .public_key
                    .as_str();
                format!("{}{}", encode(k), pk)
            },
            _ => unreachable!(),
        };
        let file_path = PathBuf::from(format!(
            "{}/{}",
            node_files_path.as_ref().to_string_lossy(),
            filename
        ));
        filenames.push(PathBuf::from(filename));
        let content = format!("\"{}\"", acc.seed);
        scoped_fs.write(file_path, content)
    });

    // TODO: implement logic for filter keys
    //   node.keystoreKeyTypes?.forEach((key_type: string) => {
    // if (DEFAULT_KEYSTORE_KEY_TYPES.includes(key_type))
    // keystore_key_types[key_type] = default_keystore_key_types[key_type];
    // });

    futures::future::try_join_all(f).await?;
    Ok(filenames)
}
