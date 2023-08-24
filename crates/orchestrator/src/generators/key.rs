use sp_core::{ecdsa, ed25519, sr25519, Pair};

use super::errors::GeneratorError;
use crate::shared::types::{Accounts, NodeAccount};
const KEY_TYPES: [&str; 3] = ["sr", "ed", "ec"];

fn generate<T: Pair>(seed: &str) -> Result<T::Pair, ()> {
    // let s = format!("//{}", seed);
    // let s: String = seed.into();
    // let s = format!("//{}{}", (&seed[..1].to_string()).to_uppercase(), &seed[1..]);
    let pair = T::Pair::from_string(&seed, None).map_err(|_| ())?;
    // let pk = pair.public();
    // println!("{:?}", pk.as_slice());

    Ok(pair)
}

pub fn generate_for_node(seed: &str) -> Result<Accounts, GeneratorError> {
    let mut accounts: Accounts = Default::default();
    for key in KEY_TYPES {
        // let public =  match key {
        let (address, public_key) = match key {
            "sr" => {
                let pair = generate::<sr25519::Pair>(&seed)
                    .map_err(|_| GeneratorError::KeyGeneration(key.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "ed" => {
                let pair = generate::<ed25519::Pair>(&seed)
                    .map_err(|_| GeneratorError::KeyGeneration(key.into(), seed.into()))?;
                (pair.public().to_string(), hex::encode(pair.public()))
            },
            "ec" => {
                let pair = generate::<ecdsa::Pair>(&seed)
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

    use std::fmt::format;

    use sp_core::crypto::{Ss58AddressFormat, Ss58AddressFormatRegistry};

    use super::*;
    #[test]
    fn a() {
        use sp_core::{crypto::Ss58Codec, ecdsa, ed25519, sr25519, Pair};
        let s = "Alice";
        let seed = format!("//{}", s);
        let pair = generate::<sr25519::Pair>(&seed).unwrap();
        println!("{s}: {}", pair.public());
        println!("{s}: {}", pair.public().to_ss58check());
        println!(
            "{s}: {}",
            pair.public()
                .to_ss58check_with_version(Ss58AddressFormatRegistry::PolkadotAccount.into())
        );
        println!("{s}: {}", hex::encode(pair.public()));
        println!(
            "{s}: {}",
            hex::encode("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY")
        );

        println!("---");
        let pair: ecdsa::Pair = generate::<ecdsa::Pair>(&seed).unwrap();
        println!("{s}: {}", pair.public());
        println!("{s}: {}", pair.public().to_ss58check());
        println!("{s}: {}", hex::encode(pair.public()));
    }
}

// zombie                     "sr_account": {
//     zombie                         "address": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
//     zombie                         "publicKey": "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d"
//     zombie                     },
