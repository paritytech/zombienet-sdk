use anyhow::anyhow;
use subxt::{
    dynamic::Value,
    ext::{codec::Encode, scale_value::value},
    tx::DynamicPayload,
    OnlineClient, PolkadotConfig,
};

/// Fetches the genesis header from a parachain node
pub async fn fetch_genesis_header(
    client: &OnlineClient<PolkadotConfig>,
) -> Result<Vec<u8>, anyhow::Error> {
    let genesis_hash = client.genesis_hash();
    let header = client
        .backend()
        .block_header(genesis_hash)
        .await?
        .ok_or_else(|| anyhow!("Failed to fetch genesis header"))?;
    Ok(header.encode())
}

/// Fetches the validation code from a parachain node
pub async fn fetch_validation_code(
    client: &OnlineClient<PolkadotConfig>,
) -> Result<Vec<u8>, anyhow::Error> {
    let code_key = sp_core::storage::well_known_keys::CODE;
    client
        .storage()
        .at_latest()
        .await?
        .fetch_raw(code_key)
        .await?
        .ok_or_else(|| anyhow!("Failed to fetch validation code"))
}

/// Creates a sudo call to deregister a validator
pub fn create_deregister_validator_call(stash_account: Value) -> DynamicPayload {
    let deregister_call = subxt::dynamic::tx(
        "ValidatorManager",
        "deregister_validators",
        vec![Value::unnamed_composite(vec![stash_account])],
    );

    subxt::dynamic::tx("Sudo", "sudo", vec![deregister_call.into_value()])
}

/// Creates a sudo call to register a validator
pub fn create_register_validator_call(stash_account: Value) -> DynamicPayload {
    let register_call = subxt::dynamic::tx(
        "ValidatorManager",
        "register_validators",
        vec![Value::unnamed_composite(vec![stash_account])],
    );

    subxt::dynamic::tx("Sudo", "sudo", vec![register_call.into_value()])
}

/// Creates a sudo batch call to register a parachain with trusted validation code
pub fn create_register_para_call(
    genesis_header: &[u8],
    validation_code: &[u8],
    para_id: u32,
    registrar_account: Value,
) -> DynamicPayload {
    let genesis_head_value = Value::from_bytes(genesis_header);
    let validation_code_value = Value::from_bytes(validation_code);
    let validation_code_for_trusted = Value::from_bytes(validation_code);

    let add_trusted_code_call = subxt::dynamic::tx(
        "Paras",
        "add_trusted_validation_code",
        vec![validation_code_for_trusted],
    );

    let force_register_call = subxt::dynamic::tx(
        "Registrar",
        "force_register",
        vec![
            registrar_account,
            Value::u128(0),
            Value::u128(para_id as u128),
            genesis_head_value,
            validation_code_value,
        ],
    );

    let calls = Value::unnamed_composite(vec![
        add_trusted_code_call.into_value(),
        force_register_call.into_value(),
    ]);

    subxt::dynamic::tx(
        "Sudo",
        "sudo",
        vec![value! {
            Utility( batch { calls: calls})
        }],
    )
}
