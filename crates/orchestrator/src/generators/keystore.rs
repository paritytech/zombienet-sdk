use std::{
    path::{Path, PathBuf},
    vec,
};

use hex::encode;
use support::{constants::THIS_IS_A_BUG, fs::FileSystem};

use super::errors::GeneratorError;
use crate::{
    generators::keystore_key_types::{parse_keystore_key_types, KeystoreKeyType},
    shared::types::NodeAccounts,
    ScopedFilesystem,
};

/// Generates keystore files for a node.
///
/// # Arguments
/// * `acc` - The node accounts containing the seed and public keys
/// * `node_files_path` - The path where keystore files will be created
/// * `scoped_fs` - The scoped filesystem for file operations
/// * `asset_hub_polkadot` - Whether this is for asset-hub-polkadot (affects aura key scheme)
/// * `keystore_key_types` - Optional list of key type specifications
///
/// If `keystore_key_types` is empty, all default key types will be generated.
/// Otherwise, only the specified key types will be generated.
pub async fn generate<'a, T>(
    acc: &NodeAccounts,
    node_files_path: impl AsRef<Path>,
    scoped_fs: &ScopedFilesystem<'a, T>,
    asset_hub_polkadot: bool,
    keystore_key_types: &[String],
) -> Result<Vec<PathBuf>, GeneratorError>
where
    T: FileSystem,
{
    // Create local keystore
    scoped_fs.create_dir_all(node_files_path.as_ref()).await?;
    let mut filenames = vec![];

    // Parse the key type specifications
    let key_types = parse_keystore_key_types(keystore_key_types, asset_hub_polkadot);

    let futures: Vec<_> = key_types
        .iter()
        .map(|key_type| {
            let filename = generate_keystore_filename(key_type, acc);
            let file_path = PathBuf::from(format!(
                "{}/{}",
                node_files_path.as_ref().to_string_lossy(),
                filename
            ));
            let content = format!("\"{}\"", acc.seed);
            (filename, scoped_fs.write(file_path, content))
        })
        .collect();

    for (filename, future) in futures {
        future.await?;
        filenames.push(PathBuf::from(filename));
    }

    Ok(filenames)
}

/// Generates the keystore filename for a given key type.
///
/// The filename format is: `{hex_encoded_key_type}{public_key}`
fn generate_keystore_filename(key_type: &KeystoreKeyType, acc: &NodeAccounts) -> String {
    let account_key = key_type.scheme.account_key();
    let pk = acc
        .accounts
        .get(account_key)
        .expect(&format!(
            "Key '{}' should be set for node {THIS_IS_A_BUG}",
            account_key
        ))
        .public_key
        .as_str();

    format!("{}{}", encode(&key_type.key_type), pk)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString, str::FromStr};

    use support::fs::in_memory::{InMemoryFile, InMemoryFileSystem};

    use super::*;
    use crate::shared::types::{NodeAccount, NodeAccounts};

    fn create_test_accounts() -> NodeAccounts {
        let mut accounts = HashMap::new();
        accounts.insert(
            "sr".to_string(),
            NodeAccount::new("sr_address", "sr_public_key"),
        );
        accounts.insert(
            "ed".to_string(),
            NodeAccount::new("ed_address", "ed_public_key"),
        );
        accounts.insert(
            "ec".to_string(),
            NodeAccount::new("ec_address", "ec_public_key"),
        );
        NodeAccounts {
            seed: "//Alice".to_string(),
            accounts,
        }
    }

    fn create_test_fs() -> InMemoryFileSystem {
        InMemoryFileSystem::new(HashMap::from([(
            OsString::from_str("/").unwrap(),
            InMemoryFile::dir(),
        )]))
    }

    #[tokio::test]
    async fn generate_creates_default_keystore_files_when_no_key_types_specified() {
        let accounts = create_test_accounts();
        let fs = create_test_fs();
        let base_dir = "/tmp/test";

        let scoped_fs = ScopedFilesystem { fs: &fs, base_dir };
        let key_types: Vec<String> = vec![];

        let res = generate(&accounts, "node1", &scoped_fs, false, &key_types).await;
        assert!(res.is_ok());

        let filenames = res.unwrap();

        assert!(filenames.len() > 10);

        let filename_strs: Vec<String> = filenames
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // Check that aura key is generated (hex of "aura" is 61757261)
        assert!(filename_strs.iter().any(|f| f.starts_with("61757261")));
        // Check that babe key is generated (hex of "babe" is 62616265)
        assert!(filename_strs.iter().any(|f| f.starts_with("62616265")));
        // Check that gran key is generated (hex of "gran" is 6772616e)
        assert!(filename_strs.iter().any(|f| f.starts_with("6772616e")));
    }

    #[tokio::test]
    async fn generate_creates_only_specified_keystore_files() {
        let accounts = create_test_accounts();
        let fs = create_test_fs();
        let base_dir = "/tmp/test";

        let scoped_fs = ScopedFilesystem { fs: &fs, base_dir };
        let key_types = vec!["audi".to_string(), "gran".to_string()];

        let res = generate(&accounts, "node1", &scoped_fs, false, &key_types).await;

        assert!(res.is_ok());

        let filenames = res.unwrap();
        assert_eq!(filenames.len(), 2);

        let filename_strs: Vec<String> = filenames
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // audi uses sr scheme by default
        assert!(filename_strs
            .iter()
            .any(|f| f.starts_with("61756469") && f.contains("sr_public_key")));
        // gran uses ed scheme by default
        assert!(filename_strs
            .iter()
            .any(|f| f.starts_with("6772616e") && f.contains("ed_public_key")));
    }

    #[tokio::test]
    async fn generate_produces_correct_keystore_files() {
        struct TestCase {
            name: &'static str,
            key_types: Vec<&'static str>,
            asset_hub_polkadot: bool,
            expected_prefix: &'static str,
            expected_public_key: &'static str,
        }

        let test_cases = vec![
            TestCase {
                name: "explicit scheme override (gran_sr)",
                key_types: vec!["gran_sr"],
                asset_hub_polkadot: false,
                expected_prefix: "6772616e", // "gran" in hex
                expected_public_key: "sr_public_key",
            },
            TestCase {
                name: "aura with asset_hub_polkadot uses ed",
                key_types: vec!["aura"],
                asset_hub_polkadot: true,
                expected_prefix: "61757261", // "aura" in hex
                expected_public_key: "ed_public_key",
            },
            TestCase {
                name: "aura without asset_hub_polkadot uses sr",
                key_types: vec!["aura"],
                asset_hub_polkadot: false,
                expected_prefix: "61757261", // "aura" in hex
                expected_public_key: "sr_public_key",
            },
            TestCase {
                name: "custom key type with explicit ec scheme",
                key_types: vec!["cust_ec"],
                asset_hub_polkadot: false,
                expected_prefix: "63757374", // "cust" in hex
                expected_public_key: "ec_public_key",
            },
        ];

        for tc in test_cases {
            let accounts = create_test_accounts();
            let fs = create_test_fs();
            let scoped_fs = ScopedFilesystem {
                fs: &fs,
                base_dir: "/tmp/test",
            };

            let key_types: Vec<String> = tc.key_types.iter().map(|s| s.to_string()).collect();
            let res = generate(
                &accounts,
                "node1",
                &scoped_fs,
                tc.asset_hub_polkadot,
                &key_types,
            )
            .await;

            assert!(
                res.is_ok(),
                "[{}] Expected Ok but got: {:?}",
                tc.name,
                res.err()
            );
            let filenames = res.unwrap();

            assert_eq!(filenames.len(), 1, "[{}] Expected 1 file", tc.name);

            let filename = filenames[0].to_string_lossy().to_string();
            assert!(
                filename.starts_with(tc.expected_prefix),
                "[{}] Expected prefix '{}', got '{}'",
                tc.name,
                tc.expected_prefix,
                filename
            );
            assert!(
                filename.contains(tc.expected_public_key),
                "[{}] Expected public key '{}' in '{}'",
                tc.name,
                tc.expected_public_key,
                filename
            );
        }
    }

    #[tokio::test]
    async fn generate_ignores_invalid_key_specs_and_uses_defaults() {
        let accounts = create_test_accounts();
        let fs = create_test_fs();
        let scoped_fs = ScopedFilesystem {
            fs: &fs,
            base_dir: "/tmp/test",
        };

        let key_types = vec![
            "invalid".to_string(), // Too long
            "xxx".to_string(),     // Too short
            "audi_xx".to_string(), // Invalid sceme
        ];

        let res = generate(&accounts, "node1", &scoped_fs, false, &key_types).await;

        assert!(res.is_ok());
        let filenames = res.unwrap();

        // Should fall back to defaults since all specs are invalid
        assert!(filenames.len() > 10);
    }
}
