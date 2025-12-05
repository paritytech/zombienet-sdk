use std::{collections::HashMap, fmt::Formatter};

use serde::{Deserialize, Serialize};

/// Supported cryptographic schemes for keystore keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyScheme {
    /// Sr25519 scheme
    Sr,
    /// Ed25519 scheme
    Ed,
    /// ECDSA scheme
    Ec,
}

impl KeyScheme {
    /// Returns the account key suffix used in `NodeAccounts` for this scheme.
    pub fn account_key(&self) -> &'static str {
        match self {
            KeyScheme::Sr => "sr",
            KeyScheme::Ed => "ed",
            KeyScheme::Ec => "ec",
        }
    }
}

impl std::fmt::Display for KeyScheme {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyScheme::Sr => write!(f, "sr"),
            KeyScheme::Ed => write!(f, "ed"),
            KeyScheme::Ec => write!(f, "ec"),
        }
    }
}

impl TryFrom<&str> for KeyScheme {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "sr" => Ok(KeyScheme::Sr),
            "ed" => Ok(KeyScheme::Ed),
            "ec" => Ok(KeyScheme::Ec),
            _ => Err(format!("Unsupported key scheme: {}", value)),
        }
    }
}

/// A parsed keystore key type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeystoreKeyType {
    /// The 4-character key type identifier (e.g., "aura", "babe", "gran").
    pub key_type: String,
    /// The cryptographic scheme to use for this key type.
    pub scheme: KeyScheme,
}

impl KeystoreKeyType {
    pub fn new(key_type: impl Into<String>, scheme: KeyScheme) -> Self {
        Self {
            key_type: key_type.into(),
            scheme,
        }
    }
}

/// Returns the default predefined key schemes for known key types.
/// Special handling for `aura` when `is_asset_hub_polkadot` is true.
fn get_predefined_schemes(is_asset_hub_polkadot: bool) -> HashMap<&'static str, KeyScheme> {
    let mut schemes = HashMap::new();

    // aura has special handling for asset-hub-polkadot
    if is_asset_hub_polkadot {
        schemes.insert("aura", KeyScheme::Ed);
    } else {
        schemes.insert("aura", KeyScheme::Sr);
    }

    schemes.insert("babe", KeyScheme::Sr);
    schemes.insert("imon", KeyScheme::Sr);
    schemes.insert("gran", KeyScheme::Ed);
    schemes.insert("audi", KeyScheme::Sr);
    schemes.insert("asgn", KeyScheme::Sr);
    schemes.insert("para", KeyScheme::Sr);
    schemes.insert("beef", KeyScheme::Ec);
    schemes.insert("nmbs", KeyScheme::Sr); // Nimbus
    schemes.insert("rand", KeyScheme::Sr); // Randomness (Moonbeam)
    schemes.insert("rate", KeyScheme::Ed); // Equilibrium rate module
    schemes.insert("acco", KeyScheme::Sr);
    schemes.insert("bcsv", KeyScheme::Sr); // BlockchainSrvc (StorageHub)
    schemes.insert("ftsv", KeyScheme::Ed); // FileTransferSrvc (StorageHub)
    schemes.insert("mixn", KeyScheme::Sr); // Mixnet

    schemes
}

/// Parses a single keystore key type specification string.
///
/// Supports two formats:
/// - Short: `audi` - creates key type with predefined default scheme (defaults to `sr` if not predefined)
/// - Long: `audi_sr` - creates key type with explicit scheme
///
/// Returns `None` if the spec is invalid or doesn't match the expected format.
fn parse_key_spec(spec: &str, predefined: &HashMap<&str, KeyScheme>) -> Option<KeystoreKeyType> {
    let spec = spec.trim();

    // Try parsing as long form first: key_type_scheme (e.g., "audi_sr")
    if let Some((key_type, scheme_str)) = spec.split_once('_') {
        if key_type.len() != 4 {
            return None;
        }

        let scheme = KeyScheme::try_from(scheme_str).ok()?;
        return Some(KeystoreKeyType::new(key_type, scheme));
    }

    // Try parsing as short form: key_type only (e.g., "audi")
    if spec.len() == 4 {
        // Look up predefined scheme; default to Sr if not found
        let scheme = predefined.get(spec).copied().unwrap_or(KeyScheme::Sr);
        return Some(KeystoreKeyType::new(spec, scheme));
    }

    None
}

/// Parses a list of keystore key type specifications.
///
/// Each spec can be in short form (`audi`) or long form (`audi_sr`).
/// Invalid specs are silently ignored.
///
/// If the resulting list is empty, returns the default keystore key types.
pub fn parse_keystore_key_types(
    specs: &[String],
    is_asset_hub_polkadot: bool,
) -> Vec<KeystoreKeyType> {
    let predefined_schemes = get_predefined_schemes(is_asset_hub_polkadot);

    let parsed: Vec<KeystoreKeyType> = specs
        .iter()
        .filter_map(|spec| parse_key_spec(spec, &predefined_schemes))
        .collect();

    if parsed.is_empty() {
        get_default_keystore_key_types(is_asset_hub_polkadot)
    } else {
        parsed
    }
}

/// Returns the default keystore key types when none are specified.
pub fn get_default_keystore_key_types(is_asset_hub_polkadot: bool) -> Vec<KeystoreKeyType> {
    let predefined_schemes = get_predefined_schemes(is_asset_hub_polkadot);
    let default_keys = [
        "aura", "babe", "imon", "gran", "audi", "asgn", "para", "beef", "nmbs", "rand", "rate",
        "mixn", "bcsv", "ftsv",
    ];

    default_keys
        .iter()
        .filter_map(|key_type| {
            predefined_schemes
                .get(*key_type)
                .map(|scheme| KeystoreKeyType::new(*key_type, *scheme))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keystore_key_types_ignores_invalid_specs() {
        let specs = vec![
            "audi".to_string(),
            "invalid".to_string(), // Too long - ignored
            "xxx".to_string(),     // Too short - ignored
            "xxxx".to_string(),    // Unknown key - defaults to sr
            "audi_xx".to_string(), // Invalid scheme - ignored
            "gran".to_string(),
        ];

        let result = parse_keystore_key_types(&specs, false);
        assert_eq!(result.len(), 3);
        assert_eq!(result[1], KeystoreKeyType::new("xxxx", KeyScheme::Sr)); // Unknown defaults to sr
        assert_eq!(result[2], KeystoreKeyType::new("gran", KeyScheme::Ed));
    }

    #[test]
    fn parse_keystore_key_types_returns_specified_keys() {
        let specs = vec!["audi".to_string(), "gran".to_string()];
        let res = parse_keystore_key_types(&specs, false);

        assert_eq!(res.len(), 2);
        assert_eq!(res[0], KeystoreKeyType::new("audi", KeyScheme::Sr));
        assert_eq!(res[1], KeystoreKeyType::new("gran", KeyScheme::Ed));
    }

    #[test]
    fn parse_keystore_key_types_mixed_short_and_long_forms() {
        let specs = vec![
            "audi".to_string(),
            "gran_sr".to_string(), // Override gran's default ed to sr
            "gran".to_string(),
            "beef".to_string(),
        ];
        let res = parse_keystore_key_types(&specs, false);

        assert_eq!(res.len(), 4);
        assert_eq!(res[0], KeystoreKeyType::new("audi", KeyScheme::Sr));
        assert_eq!(res[1], KeystoreKeyType::new("gran", KeyScheme::Sr)); // Overridden
        assert_eq!(res[2], KeystoreKeyType::new("gran", KeyScheme::Ed));
        assert_eq!(res[3], KeystoreKeyType::new("beef", KeyScheme::Ec));
    }

    #[test]
    fn parse_keystore_key_types_returns_defaults_when_empty() {
        let specs: Vec<String> = vec![];
        let res = parse_keystore_key_types(&specs, false);

        // Should return all default keys
        assert!(!res.is_empty());
        assert!(res.iter().any(|k| k.key_type == "aura"));
        assert!(res.iter().any(|k| k.key_type == "babe"));
        assert!(res.iter().any(|k| k.key_type == "gran"));
    }

    #[test]
    fn parse_keystore_key_types_allows_custom_key_with_explicit_scheme() {
        let specs = vec![
            "cust_sr".to_string(), // Custom key with explicit scheme
            "audi".to_string(),
        ];
        let result = parse_keystore_key_types(&specs, false);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], KeystoreKeyType::new("cust", KeyScheme::Sr));
        assert_eq!(result[1], KeystoreKeyType::new("audi", KeyScheme::Sr));
    }

    #[test]
    fn full_workflow_asset_hub_polkadot() {
        // For asset-hub-polkadot, aura should default to ed
        let specs = vec!["aura".to_string(), "babe".to_string()];

        let res = parse_keystore_key_types(&specs, true);

        assert_eq!(res.len(), 2);
        assert_eq!(res[0].key_type, "aura");
        assert_eq!(res[0].scheme, KeyScheme::Ed); // sr for asset-hub-polkadot

        assert_eq!(res[1].key_type, "babe");
        assert_eq!(res[1].scheme, KeyScheme::Sr);
    }

    #[test]
    fn full_workflow_custom_key_types() {
        let specs = vec![
            "aura".to_string(),    // Use default scheme
            "gran_sr".to_string(), // Override gran to use sr instead of ed
            "cust_ec".to_string(), // Custom key type with ecdsa
        ];

        let res = parse_keystore_key_types(&specs, false);

        assert_eq!(res.len(), 3);

        // aura uses default sr
        assert_eq!(res[0].key_type, "aura");
        assert_eq!(res[0].scheme, KeyScheme::Sr);

        // gran overridden to sr
        assert_eq!(res[1].key_type, "gran");
        assert_eq!(res[1].scheme, KeyScheme::Sr);

        // custom key with ec
        assert_eq!(res[2].key_type, "cust");
        assert_eq!(res[2].scheme, KeyScheme::Ec);
    }
}
