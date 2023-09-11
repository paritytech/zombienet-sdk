use sp_core::{ecdsa, ed25519, sr25519, Pair};

use super::errors::GeneratorError;
use crate::shared::types::{Accounts, NodeAccount};
const KEY_TYPES: [&str; 3] = ["sr", "ed", "ec"];

fn generate<T: Pair>(seed: &str) -> Result<T::Pair, ()> {
    let pair = T::Pair::from_string(seed, None).map_err(|_| ())?;
    Ok(pair)
}

pub fn generate_for_node(seed: &str) -> Result<Accounts, GeneratorError> {
    let mut accounts: Accounts = Default::default();
    for key in KEY_TYPES {
        let (address, public_key) = match key {
            "sr" => {
                let pair = generate::<sr25519::Pair>(seed)
                    .map_err(|_| GeneratorError::KeyGeneration(key.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "ed" => {
                let pair = generate::<ed25519::Pair>(seed)
                    .map_err(|_| GeneratorError::KeyGeneration(key.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "ec" => {
                let pair = generate::<ecdsa::Pair>(seed)
                    .map_err(|_| GeneratorError::KeyGeneration(key.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            _ => return Err(GeneratorError::KeyGeneration(key.into(), seed.into())),
        };
        accounts.insert(key.into(), NodeAccount::new(address, public_key));
    }
    Ok(accounts)
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn generate_for_alice() {
        use sp_core::crypto::Ss58Codec;
        let s = "Alice";
        let seed = format!("//{}", s);

        let pair = generate::<sr25519::Pair>(&seed).unwrap();
        assert_eq!(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            pair.public().to_ss58check()
        );

        let pair = generate::<ecdsa::Pair>(&seed).unwrap();
        assert_eq!(
            "0x020a1091341fe5664bfa1782d5e04779689068c916b04cb365ec3153755684d9a1",
            format!("0x{}", hex::encode(pair.public()))
        );

        let pair = generate::<ed25519::Pair>(&seed).unwrap();
        assert_eq!(
            "5FA9nQDVg267DEd8m1ZypXLBnvN7SFxYwV7ndqSYGiN9TTpu",
            pair.public().to_ss58check()
        );
    }
}
