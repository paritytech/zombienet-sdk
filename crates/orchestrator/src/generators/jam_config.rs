use configuration::types::JamNodeMode;
use serde::{Deserialize, Serialize};

use crate::{generators::errors::GeneratorError, network_spec::jamchain::JamchainSpec};

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
    };

    Ok(gen_config)
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
        }
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
