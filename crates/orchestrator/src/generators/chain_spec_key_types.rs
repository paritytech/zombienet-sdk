use std::collections::HashMap;

use super::keystore_key_types::KeyScheme;

/// A parsed chain spec session key type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSpecKeyType {
    /// The key type name as it appears in the chain spec (e.g., "aura", "grandpa", "babe").
    pub key_name: String,
    /// The cryptographic scheme to use for this key type.
    pub scheme: KeyScheme,
}

impl ChainSpecKeyType {
    pub fn new(key_name: impl Into<String>, scheme: KeyScheme) -> Self {
        Self {
            key_name: key_name.into(),
            scheme,
        }
    }
}

/// Returns the default predefined key schemes for known chain spec key types.
/// Special handling for `aura` when `is_asset_hub_polkadot` is true.
fn get_predefined_schemes(is_asset_hub_polkadot: bool) -> HashMap<&'static str, KeyScheme> {
    let mut schemes = HashMap::new();

    // aura has special handling for asset-hub-polkadot
    if is_asset_hub_polkadot {
        schemes.insert("aura", KeyScheme::Ed);
    } else {
        schemes.insert("aura", KeyScheme::Sr);
    }

    // SR25519 keys
    schemes.insert("babe", KeyScheme::Sr);
    schemes.insert("im_online", KeyScheme::Sr);
    schemes.insert("parachain_validator", KeyScheme::Sr);
    schemes.insert("authority_discovery", KeyScheme::Sr);
    schemes.insert("para_validator", KeyScheme::Sr);
    schemes.insert("para_assignment", KeyScheme::Sr);
    schemes.insert("nimbus", KeyScheme::Sr);
    schemes.insert("vrf", KeyScheme::Sr);

    // ED25519 keys
    schemes.insert("grandpa", KeyScheme::Ed);

    // ECDSA keys
    schemes.insert("beefy", KeyScheme::Ec);

    schemes
}

/// Parses a single chain spec key type specification string.
///
/// Supports two formats:
/// - Short: `aura` - uses predefined default scheme (defaults to `sr` if not predefined)
/// - Long: `aura_sr` - uses explicit scheme
///
/// Returns `None` if the spec is invalid (e.g., invalid scheme).
pub fn parse_key_spec(
    spec: &str,
    predefined: &HashMap<&str, KeyScheme>,
) -> Option<ChainSpecKeyType> {
    let spec = spec.trim();

    if spec.is_empty() {
        return None;
    }

    if let Some((key_name, scheme_str)) = spec.rsplit_once('_') {
        if let Ok(scheme) = KeyScheme::try_from(scheme_str) {
            return Some(ChainSpecKeyType::new(key_name, scheme));
        }
        // If not a valid scheme, define whole string as the key name
    }

    let scheme = predefined.get(spec).copied().unwrap_or(KeyScheme::Sr);
    Some(ChainSpecKeyType::new(spec, scheme))
}

/// Parses a list of chain spec key type specifications.
///
/// Each spec can be in short form (`aura`) or long form (`aura_sr`).
/// Invalid specs are silently ignored.
///
/// If the input list is empty, returns `None` to indicate that default
/// chain spec behavior should be used.
pub fn parse_chain_spec_key_types<T: AsRef<str>>(
    specs: &[T],
    is_asset_hub_polkadot: bool,
) -> Option<Vec<ChainSpecKeyType>> {
    if specs.is_empty() {
        return None;
    }

    let predefined_schemes = get_predefined_schemes(is_asset_hub_polkadot);

    let parsed: Vec<ChainSpecKeyType> = specs
        .iter()
        .filter_map(|spec| parse_key_spec(spec.as_ref(), &predefined_schemes))
        .collect();

    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

pub fn get_default_chain_spec_key_types(is_asset_hub_polkadot: bool) -> Vec<ChainSpecKeyType> {
    let predefined_schemes = get_predefined_schemes(is_asset_hub_polkadot);
    let default_keys = [
        "babe",
        "im_online",
        "parachain_validator",
        "authority_discovery",
        "para_validator",
        "para_assignment",
        "aura",
        "nimbus",
        "vrf",
        "grandpa",
        "beefy",
    ];

    default_keys
        .iter()
        .filter_map(|key_name| {
            predefined_schemes
                .get(*key_name)
                .map(|scheme| ChainSpecKeyType::new(*key_name, *scheme))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chain_spec_key_types_returns_none_when_empty() {
        let specs: Vec<String> = vec![];
        let result = parse_chain_spec_key_types(&specs, false);
        assert!(result.is_none());
    }

    #[test]
    fn parse_chain_spec_key_types_parses_short_form() {
        let specs = vec!["aura".to_string(), "grandpa".to_string()];
        let result = parse_chain_spec_key_types(&specs, false).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ChainSpecKeyType::new("aura", KeyScheme::Sr));
        assert_eq!(result[1], ChainSpecKeyType::new("grandpa", KeyScheme::Ed));
    }

    #[test]
    fn parse_chain_spec_key_types_parses_long_form() {
        let specs = vec![
            "aura_ed".to_string(),    // Override aura to use ed
            "grandpa_sr".to_string(), // Override grandpa to use sr
            "custom_ec".to_string(),  // Custom key with ec
        ];
        let result = parse_chain_spec_key_types(&specs, false).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], ChainSpecKeyType::new("aura", KeyScheme::Ed));
        assert_eq!(result[1], ChainSpecKeyType::new("grandpa", KeyScheme::Sr));
        assert_eq!(result[2], ChainSpecKeyType::new("custom", KeyScheme::Ec));
    }

    #[test]
    fn parse_chain_spec_key_types_mixed_forms() {
        let specs = vec![
            "aura".to_string(),       // Short form - uses default sr
            "grandpa_sr".to_string(), // Long form - override to sr
            "babe".to_string(),       // Short form - uses default sr
        ];
        let result = parse_chain_spec_key_types(&specs, false).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], ChainSpecKeyType::new("aura", KeyScheme::Sr));
        assert_eq!(result[1], ChainSpecKeyType::new("grandpa", KeyScheme::Sr));
        assert_eq!(result[2], ChainSpecKeyType::new("babe", KeyScheme::Sr));
    }

    #[test]
    fn parse_chain_spec_key_types_asset_hub_polkadot() {
        let specs = vec!["aura".to_string(), "babe".to_string()];

        let result = parse_chain_spec_key_types(&specs, false).unwrap();
        assert_eq!(result[0].scheme, KeyScheme::Sr);

        let result = parse_chain_spec_key_types(&specs, true).unwrap();
        assert_eq!(result[0].scheme, KeyScheme::Ed);
    }

    #[test]
    fn parse_chain_spec_key_types_unknown_key_defaults_to_sr() {
        let specs = vec!["unknown_key".to_string()];
        let result = parse_chain_spec_key_types(&specs, false).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            ChainSpecKeyType::new("unknown_key", KeyScheme::Sr)
        );
    }

    #[test]
    fn parse_chain_spec_key_types_handles_underscore_in_key_name() {
        let specs = vec![
            "im_online".to_string(),        // Known key with underscore
            "para_validator".to_string(),   // Known key with underscore
            "my_custom_key_sr".to_string(), // Custom key with underscores and explicit scheme
        ];
        let result = parse_chain_spec_key_types(&specs, false).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], ChainSpecKeyType::new("im_online", KeyScheme::Sr));
        assert_eq!(
            result[1],
            ChainSpecKeyType::new("para_validator", KeyScheme::Sr)
        );
        assert_eq!(
            result[2],
            ChainSpecKeyType::new("my_custom_key", KeyScheme::Sr)
        );
    }
}
