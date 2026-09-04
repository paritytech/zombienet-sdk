use configuration::types::JamNodeMode;
use serde::{Deserialize, Serialize};

use crate::{
    generators::{chain_spec::merge, errors::GeneratorError},
    network_spec::jamchain::JamchainSpec,
};

/// Generate config file used to get the chain-spec
/// Using this json format:
/// {
//     "id": "dev",
//     "genesis_validators": [
//         {
//             "peer_id": "dev:0",
//             "bandersnatch": "dev:0",
//             "net_addr": "127.0.0.1:40000"
//         },
//         {
//             "peer_id": "dev:1",
//             "bandersnatch": "dev:1",
//             "net_addr": "127.0.0.1:40001"
//         },
//         {
//             "peer_id": "dev:2",
//             "bandersnatch": "dev:2",
//             "net_addr": "127.0.0.1:40002"
//         },
//         {
//             "peer_id": "dev:3",
//             "bandersnatch": "dev:3",
//             "net_addr": "127.0.0.1:40003"
//         },
//         {
//             "peer_id": "dev:4",
//             "bandersnatch": "dev:4",
//             "net_addr": "127.0.0.1:40004"
//         },
//         {
//             "peer_id": "dev:5",
//             "bandersnatch": "dev:5",
//             "net_addr": "127.0.0.1:40005"
//         }
//     ]
// }
///

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenesisValidator {
    pub peer_id: String,
    pub bandersnatch: String,
    pub net_addr: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub id: String,
    pub genesis_validators: Vec<GenesisValidator>,
    /// Whatever the genesis overrides add beyond the generated keys, passed to `gen-spec` as is.
    #[serde(flatten)]
    pub overrides: serde_json::Map<String, serde_json::Value>,
}

pub fn generate(jam_spec: &JamchainSpec) -> Result<GenesisConfig, GeneratorError> {
    let genesis_validators = jam_spec
        .nodes
        .iter()
        .filter_map(|n| {
            if let JamNodeMode::Validator = n.mode {
                Some(GenesisValidator {
                    peer_id: n.peer_id.clone(),
                    bandersnatch: n
                        .accounts
                        .accounts
                        .get("bandersnatch")
                        .expect("bandersnatch should be present.")
                        .public_key
                        .clone(),
                    // The genesis validator set doubles as the p2p address book, so this
                    // must be the port the node actually listens on (`--port`), not the
                    // rpc one.
                    net_addr: format!("127.0.0.1:{}", n.port.0),
                })
            } else {
                None
            }
        })
        .collect();

    let gen_config = GenesisConfig {
        id: jam_spec.id.as_str().into(),
        genesis_validators,
        overrides: Default::default(),
    };

    let Some(overrides) = &jam_spec.genesis_overrides else {
        return Ok(gen_config);
    };
    // Merged as JSON and read back, so the generated keys keep their place in the file.
    let mut merged = serde_json::to_value(gen_config).map_err(encode_error)?;
    merge(&mut merged, overrides);
    serde_json::from_value(merged).map_err(encode_error)
}

fn encode_error(error: serde_json::Error) -> GeneratorError {
    GeneratorError::EncodeDecodeError(error.to_string())
}

#[cfg(test)]
mod tests {
    use configuration::types::Chain;

    use super::*;
    use crate::{
        network_spec::jamnode::JamNodeSpec,
        shared::types::{NodeAccount, NodeAccounts, ParkedPort},
    };

    fn jam_node(name: &str, mode: JamNodeMode, p2p_port: u16, rpc_port: u16) -> JamNodeSpec {
        JamNodeSpec {
            name: name.to_string(),
            mode,
            peer_id: format!("peer-{name}"),
            accounts: NodeAccounts {
                seed: name.to_string(),
                accounts: [(
                    "bandersnatch".to_string(),
                    NodeAccount::new(format!("addr-{name}"), format!("pub-{name}")),
                )]
                .into(),
            },
            port: ParkedPort(p2p_port, Default::default()),
            rpc_port: ParkedPort(rpc_port, Default::default()),
            ..Default::default()
        }
    }

    fn jam_spec(nodes: Vec<JamNodeSpec>) -> JamchainSpec {
        JamchainSpec {
            id: Chain::try_from("dev").unwrap(),
            default_command: None,
            default_image: None,
            default_resources: None,
            default_args: vec![],
            chain_spec_command: String::new(),
            nodes,
            genesis_overrides: None,
        }
    }

    /// Exactly what `serde_json::to_string_pretty` wrote for a one-validator network before the
    /// overrides existed. Every existing user's `jam_config.json` is this file.
    const ALICE_ALONE: &str = r#"{
  "id": "dev",
  "genesis_validators": [
    {
      "peer_id": "peer-alice",
      "bandersnatch": "pub-alice",
      "net_addr": "127.0.0.1:40001"
    }
  ]
}"#;

    fn alice_alone(genesis_overrides: Option<serde_json::Value>) -> JamchainSpec {
        JamchainSpec {
            genesis_overrides,
            ..jam_spec(vec![jam_node("alice", JamNodeMode::Validator, 40001, 9944)])
        }
    }

    fn written(spec: &JamchainSpec) -> String {
        serde_json::to_string_pretty(&generate(spec).unwrap()).unwrap()
    }

    /// The overrides are how a caller puts services, authorizer queues and privileges into
    /// genesis; the generator knows nothing about those keys and must pass every one of them
    /// through, next to what it generated itself.
    #[test]
    fn genesis_overrides_are_added_to_the_generated_config() {
        let overrides = serde_json::json!({
            "services": [{
                "id": 5,
                "code": "/blobs/parasim-service.jam",
                "balance": "18446744073709551615",
            }],
            "auth_queues": { "0": "2bbda8cb" },
            "assigners": { "0": 5 },
            "privileges": { "bless": 0, "assign": { "0": 5 } },
        });

        let config: serde_json::Value =
            serde_json::from_str(&written(&alice_alone(Some(overrides.clone())))).unwrap();

        for key in ["services", "auth_queues", "assigners", "privileges"] {
            assert_eq!(
                config[key], overrides[key],
                "{key} did not reach the config"
            );
        }
        assert_eq!(config["id"], "dev");
        assert_eq!(
            config["genesis_validators"][0]["net_addr"],
            "127.0.0.1:40001"
        );
    }

    /// Nobody who does not use the overrides should see their file change — not even in key
    /// order, which is what a naive detour through `serde_json::Value` would reshuffle.
    #[test]
    fn without_overrides_the_config_is_written_exactly_as_before() {
        assert_eq!(written(&alice_alone(None)), ALICE_ALONE);
        assert_eq!(
            written(&alice_alone(Some(serde_json::json!({})))),
            ALICE_ALONE
        );
    }

    // The nodes dial each other using the addresses in the genesis validator set, so
    // pointing them at the rpc port silently splits the network.
    #[test]
    fn genesis_validators_are_addressed_by_their_p2p_port() {
        let spec = jam_spec(vec![
            jam_node("alice", JamNodeMode::Validator, 40001, 9944),
            jam_node("bob", JamNodeMode::Validator, 40002, 9945),
        ]);

        let config = generate(&spec).unwrap();

        let addrs: Vec<&str> = config
            .genesis_validators
            .iter()
            .map(|v| v.net_addr.as_str())
            .collect();
        assert_eq!(addrs, vec!["127.0.0.1:40001", "127.0.0.1:40002"]);
    }

    #[test]
    fn only_validators_are_included_in_genesis() {
        let spec = jam_spec(vec![
            jam_node("alice", JamNodeMode::Validator, 40001, 9944),
            jam_node("dave", JamNodeMode::Ordinary, 40002, 9945),
            jam_node("eve", JamNodeMode::Proxy, 40003, 9946),
        ]);

        let config = generate(&spec).unwrap();

        assert_eq!(config.id, "dev");
        assert_eq!(config.genesis_validators.len(), 1);
        assert_eq!(config.genesis_validators[0].peer_id, "peer-alice");
    }
}
