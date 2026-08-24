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
    pub genesis_validators: Vec<GenesisValidator>
}

pub fn generate(jam_spec: &JamchainSpec ) -> Result<GenesisConfig, GeneratorError>
{
    let genesis_validators = jam_spec.nodes.iter().filter_map(|n| {
        if let JamNodeMode::Validator = n.mode {
            Some(GenesisValidator {
                peer_id: n.peer_id.clone(),
                bandersnatch: n.accounts.accounts.get("bandersnatch").expect("bandersnatch should be present.").public_key.clone(),
                net_addr: format!("127.0.0.1:{}", n.rpc_port.0),
            })
        } else {
            None
        }
    }).collect();

    let gen_config = GenesisConfig {
        id: jam_spec.id.as_str().into(),
        genesis_validators,
    };

    Ok(gen_config)
}