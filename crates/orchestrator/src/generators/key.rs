use sp_core::{crypto::SecretStringError, ecdsa, ed25519, sr25519, Pair};

use super::errors::GeneratorError;
use crate::shared::types::{Accounts, NodeAccount};
const KEYS: [&str; 4] = ["sr", "sr_stash", "ed", "ec"];

pub fn generate_pair<T: Pair>(seed: &str) -> Result<T::Pair, SecretStringError> {
    let pair = T::Pair::from_string(seed, None)?;
    Ok(pair)
}

pub fn generate(seed: &str) -> Result<Accounts, GeneratorError> {
    let mut accounts: Accounts = Default::default();
    for k in KEYS {
        let (address, public_key) = match k {
            "sr" => {
                let pair = generate_pair::<sr25519::Pair>(seed)
                    .map_err(|_| GeneratorError::KeyGeneration(k.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "sr_stash" => {
                let pair = generate_pair::<sr25519::Pair>(&format!("{}/stash", seed))
                    .map_err(|_| GeneratorError::KeyGeneration(k.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "ed" => {
                let pair = generate_pair::<ed25519::Pair>(seed)
                    .map_err(|_| GeneratorError::KeyGeneration(k.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "ec" => {
                let pair = generate_pair::<ecdsa::Pair>(seed)
                    .map_err(|_| GeneratorError::KeyGeneration(k.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            _ => unreachable!(),
        };
        accounts.insert(k.into(), NodeAccount::new(address, public_key));
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

        let pair = generate_pair::<sr25519::Pair>(&seed).unwrap();
        assert_eq!(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            pair.public().to_ss58check()
        );

        let pair = generate_pair::<ecdsa::Pair>(&seed).unwrap();
        assert_eq!(
            "0x020a1091341fe5664bfa1782d5e04779689068c916b04cb365ec3153755684d9a1",
            format!("0x{}", hex::encode(pair.public()))
        );

        let pair = generate_pair::<ed25519::Pair>(&seed).unwrap();
        assert_eq!(
            "5FA9nQDVg267DEd8m1ZypXLBnvN7SFxYwV7ndqSYGiN9TTpu",
            pair.public().to_ss58check()
        );
    }

    #[test]
    fn generate_pair_invalid_should_fail() {
        let s = "Alice";
        let seed = s.to_string();

        let pair = generate_pair::<sr25519::Pair>(&seed);
        assert!(pair.is_err());
    }

    #[test]
    fn generate_invalid_should_fail() {
        let s = "Alice";
        let seed = s.to_string();

        let pair = generate(&seed);
        assert!(pair.is_err());
        assert!(matches!(pair, Err(GeneratorError::KeyGeneration(_, _))));
    }

    #[test]
    fn generate_work() {
        let s = "Alice";
        let seed = format!("//{}", s);

        let pair = generate(&seed).unwrap();
        let sr = pair.get("sr").unwrap();
        let sr_stash = pair.get("sr_stash").unwrap();
        let ed = pair.get("ed").unwrap();
        let ec = pair.get("ec").unwrap();
        assert_eq!(
            sr.address,
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
        );
        assert_eq!(
            sr_stash.address,
            "5DZnGRAr28KP4GvbuxW2cBNo9Aodcm4QKUMj3Zqj67YjYStr"
        );
        assert_eq!(
            ed.address,
            "5FA9nQDVg267DEd8m1ZypXLBnvN7SFxYwV7ndqSYGiN9TTpu"
        );
        assert_eq!(
            format!("0x{}", ec.public_key),
            "0x020a1091341fe5664bfa1782d5e04779689068c916b04cb365ec3153755684d9a1"
        );
    }
}
