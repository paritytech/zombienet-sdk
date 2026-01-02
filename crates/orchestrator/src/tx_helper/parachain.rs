use anyhow::anyhow;
use subxt::{dynamic::Value, tx::DynamicPayload, OnlineClient, PolkadotConfig};

/// Fetches the genesis header from a parachain node
pub async fn fetch_genesis_header(
	client: &OnlineClient<PolkadotConfig>,
) -> Result<Vec<u8>, anyhow::Error> {
	use subxt::ext::codec::Encode;
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
	let deregister_call = Value::named_composite([
		("deregister_validators", Value::named_composite([
			("validators", stash_account)
		]))
	]);
	
	subxt::dynamic::tx(
		"Sudo",
		"sudo",
		vec![Value::named_composite([
			("ValidatorManager", deregister_call)
		])],
	)
}

/// Creates a sudo call to register a validator
pub fn create_register_validator_call(stash_account: Value) -> DynamicPayload {
	let register_call = Value::named_composite([
		("register_validators", Value::named_composite([
			("validators", stash_account)
		]))
	]);
	
	subxt::dynamic::tx(
		"Sudo",
		"sudo",
		vec![Value::named_composite([
			("ValidatorManager", register_call)
		])],
	)
}

/// Creates a sudo batch call to register a parachain with trusted validation code
pub fn create_register_para_call(
	genesis_header: Vec<u8>,
	validation_code: Vec<u8>,
	para_id: u32,
	registrar_account: Value,
) -> DynamicPayload {
	let genesis_head_value = Value::from_bytes(&genesis_header);
	let validation_code_value = Value::from_bytes(&validation_code);
	let validation_code_for_trusted = Value::from_bytes(&validation_code);

	let add_trusted_code_call = Value::named_composite([
		("Paras", Value::named_composite([
			("add_trusted_validation_code", Value::named_composite([
				("validation_code", validation_code_for_trusted)
			]))
		]))
	]);

	let force_register_call = Value::named_composite([
		("Registrar", Value::named_composite([
			("force_register", Value::named_composite([
				("who", registrar_account),
				("deposit", Value::u128(0)),
				("id", Value::u128(para_id as u128)),
				("genesis_head", genesis_head_value),
				("validation_code", validation_code_value)
			]))
		]))
	]);

	let calls = Value::unnamed_composite(vec![add_trusted_code_call, force_register_call]);

	subxt::dynamic::tx(
		"Sudo",
		"sudo",
		vec![Value::named_composite([
			("Utility", Value::named_composite([
				("batch", Value::named_composite([
					("calls", calls)
				]))
			]))
		])],
	)
}
