use array_bytes::bytes2hex;
use codec::{Decode, Encode};
use serde_json::json;
use sp_core::crypto::AccountId32;
use support::substorage::storage_value_key;

use crate::generators::errors::GeneratorError;

// Extracted and simplified from polkadot-sdk

/// Index of the validator is used as a lightweight replacement of the `ValidatorId` when
/// appropriate.
#[derive(PartialEq, Clone, Encode, Decode, Debug)]
pub struct ValidatorIndex(pub u32);

/// Simple index type with which we can count sessions.
pub type SessionIndex = u32;

/// The unique (during session) index of a validator group.
#[derive(Encode, Decode, Default, Clone, Debug, PartialEq)]
pub struct GroupIndex(pub u32);

#[derive(Clone, Encode, Decode, Debug, PartialEq)]
pub struct SessionInfo {
    /// **** New in v2 ******
    /// All the validators actively participating in parachain consensus.
    /// Indices are into the broader validator set.
    pub active_validator_indices: Vec<ValidatorIndex>,
    /// A secure random seed for the session, gathered from BABE.
    pub random_seed: [u8; 32],
    /// The amount of sessions to keep for disputes.
    pub dispute_period: SessionIndex,

    /// **** Old fields *****
    /// Validators in canonical ordering.
    ///
    /// NOTE: There might be more authorities in the current session, than `validators`
    /// participating in parachain consensus. See
    /// [`max_validators`](https://github.com/paritytech/polkadot/blob/a52dca2be7840b23c19c153cf7e110b1e3e475f8/runtime/parachains/src/configuration.rs#L148).
    ///
    /// `SessionInfo::validators` will be limited to `max_validators` when set.
    pub validators: Vec<AccountId32>,
    /// Validators' authority discovery keys for the session in canonical ordering.
    ///
    /// NOTE: The first `validators.len()` entries will match the corresponding validators in
    /// `validators`, afterwards any remaining authorities can be found. This is any authorities
    /// not participating in parachain consensus - see
    /// [`max_validators`](https://github.com/paritytech/polkadot/blob/a52dca2be7840b23c19c153cf7e110b1e3e475f8/runtime/parachains/src/configuration.rs#L148)
    pub discovery_keys: Vec<AccountId32>,
    /// The assignment keys for validators.
    ///
    /// NOTE: There might be more authorities in the current session, than validators participating
    /// in parachain consensus. See
    /// [`max_validators`](https://github.com/paritytech/polkadot/blob/a52dca2be7840b23c19c153cf7e110b1e3e475f8/runtime/parachains/src/configuration.rs#L148).
    ///
    pub assignment_keys: Vec<AccountId32>,
    /// Validators in shuffled ordering - these are the validator groups as produced
    /// by the `Scheduler` module for the session and are typically referred to by
    /// `GroupIndex`.
    pub validator_groups: Vec<Vec<ValidatorIndex>>,
    /// The number of availability cores used by the protocol during this session.
    pub n_cores: u32,
    /// The zeroth delay tranche width.
    pub zeroth_delay_tranche_width: u32,
    /// The number of samples we do of `relay_vrf_modulo`.
    pub relay_vrf_modulo_samples: u32,
    /// The number of delay tranches in total.
    pub n_delay_tranches: u32,
    /// How many slots (BABE / SASSAFRAS) must pass before an assignment is considered a
    /// no-show.
    pub no_show_slots: u32,
    /// The number of validators needed to approve a block.
    pub needed_approvals: u32,
}

pub fn generate_session_0_overrides(
    raw_spec: &serde_json::Value,
    num_genesis_cores: u32,
) -> Result<serde_json::Value, GeneratorError> {
    let mut overrides = json!({});
    // get current session 0
    let sessions_prefix = storage_value_key(&b"ParaSessionInfo"[..], b"Sessions");
    let session_0_key = format!(
        "{}{}",
        bytes2hex("0x", &sessions_prefix),
        bytes2hex("", 0_u32.encode())
    );

    let current_value = &raw_spec["genesis"]["raw"]["top"][&session_0_key];
    let Some(current_value_inner) = current_value.as_str() else {
        return Err(GeneratorError::OverridingRawSpec(format!(
            "Session_0 keys {} is missing (in genesis.raw.top)",
            session_0_key
        )));
    };

    let encoded = hex::decode(&current_value_inner[2..]).map_err(|e| {
        GeneratorError::EncodeDecodeError(format!(
            "Error decoding hex: {}, err: {e}",
            current_value_inner
        ))
    })?;
    let mut session: SessionInfo = SessionInfo::decode(&mut encoded.as_slice()).map_err(|e| {
        GeneratorError::EncodeDecodeError(format!("Error decoding scale: {:?}, err: {e}", encoded))
    })?;

    // clone keys
    session.assignment_keys = session.validators.clone();
    session.discovery_keys = session.validators.clone();

    // generate validator groups

    // some checks first
    if num_genesis_cores >= session.validators.len() as u32 {
        return Err(GeneratorError::InvariantError(format!("Num cores in genesis {num_genesis_cores} should be less than the num of validators ({})", session.validators.len())));
    }

    let groups = genetate_groups(session.validators.len() as u32, num_genesis_cores);
    session.validator_groups = groups.clone();
    session.n_cores = num_genesis_cores;

    // done with session
    let session_0_value = bytes2hex("0x", session.encode());
    overrides[session_0_key] = json!(session_0_value);

    // paraScheduler.validatorGroups: Vec<Vec<u32>>
    let para_scheduler_validator_groups_key = bytes2hex(
        "0x",
        storage_value_key(&b"ParaScheduler"[..], b"ValidatorGroups"),
    );

    overrides[para_scheduler_validator_groups_key] = json!(bytes2hex("0x", groups.encode()));

    Ok(overrides)
}

fn genetate_groups(num_validators: u32, num_cores: u32) -> Vec<Vec<ValidatorIndex>> {
    let iter = std::iter::repeat(vec![]).take(num_cores as usize);
    let mut groups: Vec<Vec<ValidatorIndex>> = Vec::from_iter(iter);
    for i in 0..num_validators {
        let index = i % num_cores;
        let group = groups.get_mut(index as usize).expect(&format!(
            "Group index {index} should be part of groups. qed"
        ));
        group.push(ValidatorIndex(i));
    }

    groups
}

#[cfg(test)]
mod test {
    use tracing::debug;

    use super::*;

    #[test]
    fn decode_encode_should_work() {
        use support::substorage::storage_value_key;

        let k = storage_value_key(&b"ParaSessionInfo"[..], b"Sessions");
        debug!("k: {}{}", bytes2hex("", &k), bytes2hex("", 0_u32.encode()));

        let encoded = hex::decode( "1003000000010000000000000002000000abc3f086f5ac20eaab792c75933b2e196307835a61a955be82aa63bc0ff9617a06000000108eaf04151687736326c9fea17e25fc5287613693c912909cb226aa4794f26a48d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d90b5ab205c6974c9ea841be688864633dc9ca8a357843eeacf2314649965fe22306721211d5404bd9da88e0204360a1a9ab8b87c66c1bc2fcdd37f3c2222cc20000000000000000000000000000000010000000100000000000000").unwrap();
        let mut session: SessionInfo = SessionInfo::decode(&mut encoded.as_slice()).unwrap();

        debug!("{session:?}");

        session.assignment_keys = session.validators.clone();
        session.discovery_keys = session.validators.clone();

        let encoded = session.encode();
        debug!("{}", bytes2hex("", &encoded));

        let session_modified = SessionInfo::decode(&mut &encoded[..]).unwrap();

        debug!("{session_modified:?}");
    }

    #[test]
    fn val_groups() {
        let num_cores = 3_u32;
        let validators = ["abc", "cds", "qwe", "eds"];
        let groups = genetate_groups(validators.len() as u32, num_cores);

        debug!("{:?}", groups);
        assert_eq!(groups.len(), num_cores as usize);
    }

    #[test]
    fn generate_should_work() {
        let sessions_prefix = storage_value_key(&b"ParaSessionInfo"[..], b"Sessions");
        let session_0_key = format!(
            "{}{}",
            bytes2hex("0x", &sessions_prefix),
            bytes2hex("", 0_u32.encode())
        );

        let session_value = "0x1003000000010000000000000002000000abc3f086f5ac20eaab792c75933b2e196307835a61a955be82aa63bc0ff9617a06000000108eaf04151687736326c9fea17e25fc5287613693c912909cb226aa4794f26a48d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d90b5ab205c6974c9ea841be688864633dc9ca8a357843eeacf2314649965fe22306721211d5404bd9da88e0204360a1a9ab8b87c66c1bc2fcdd37f3c2222cc20000000000000000000000000000000010000000100000000000000";
        let mock_spec = json!({
            "genesis": {
                "raw": {
                    "top": {
                        session_0_key: session_value
                    }
                }
            }
        });

        debug!("mock {:?}", mock_spec);

        let overrides = generate_session_0_overrides(&mock_spec, 3).unwrap();
        debug!("{:?}", overrides);
    }
}
