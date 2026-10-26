//! Build the JIP-4 chain spec of a JAM chain in process, with `jam-chainspec`.
//!
//! The chain id, the genesis validators (the network's validator nodes) and the protocol
//! parameters always come from zombienet; services, auth queues and privileges from the
//! network's [`JamGenesisConfig`], layered over its optional native `config_file`. With the
//! defaults on, the genesis has the bootstrap service under id 0, hosting the null authorizer,
//! and every core's auth queue filled with the null authorizer (the same genesis polkajam's own
//! `gen-spec` writes).

use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

use configuration::{
    types::{AssetLocation, JamProtocolParameterType},
    JamAuthorizer, JamBalance, JamGenesisConfig, JAM_BOOTSTRAP_SERVICE_ID,
};
use jam_chainspec::{Blob, ChainSpec, ChainSpecConfig, GenesisBalance, GenesisService};
use jam_std_common::hash_raw;
use jam_types::{hex, AuthorizerHash, Balance, ProtocolParameters, VALS_PER_CORE};
use serde_json::json;
use tracing::info;

use crate::{
    generators::{
        errors::GeneratorError,
        jam_blobs::{self, NULL_AUTHORIZER_BLOB},
        jam_validators::{self, GenesisValidator},
    },
    network_spec::jamchain::JamchainSpec,
};

/// The protocol parameters and the bandersnatch ring parameters are process-wide, so only one
/// spec is built at a time.
static BUILD_LOCK: Mutex<()> = Mutex::new(());

/// The ring parameters are sized once, by the first spec built, so every spec built by this
/// process must use the same protocol parameters.
static APPLIED_PARAMETERS: OnceLock<ProtocolParameters> = OnceLock::new();

/// A spec built in process.
pub struct BuiltSpec {
    /// The JIP-4 chain spec.
    pub spec: String,
    /// What went into the spec, for the network's `jam_genesis.json`.
    pub summary: serde_json::Value,
}

/// Build the chain spec of `jam_spec` with the genesis `genesis`.
pub async fn build(
    jam_spec: &JamchainSpec,
    genesis: &JamGenesisConfig,
) -> Result<BuiltSpec, GeneratorError> {
    let assets = fetch_assets(genesis).await?;
    let parameters = protocol_parameters(&jam_spec.protocol_params_type, genesis)?;
    let validators = jam_validators::genesis_validators(jam_spec);
    check_validator_count(
        &jam_spec.protocol_params_type,
        genesis,
        &parameters,
        &validators,
    )?;

    let bootstrap = if genesis.with_defaults() {
        let bootstrap = bootstrap_service(genesis, &assets)?;
        info!(
            "🧰 JAM bootstrap service: {} (code hash 0x{})",
            bootstrap.source,
            hex::to_hex(&hash_raw(&bootstrap.code))
        );
        Some(bootstrap)
    } else {
        None
    };

    let config = base_config(jam_spec.id.as_str(), genesis, &validators, &assets)?;
    let mut config = apply_genesis(config, genesis, &assets);
    config.protocol_parameters = Some(parameters);
    if let Some(bootstrap) = &bootstrap {
        config = with_bootstrap_defaults(config, bootstrap.code.clone(), parameters.core_count);
    }

    let summary = summary(genesis, &config, bootstrap.as_ref(), &parameters);
    let spec = tokio::task::spawn_blocking(move || build_spec(config))
        .await
        .map_err(|e| GeneratorError::ChainSpecGeneration(e.to_string()))??;

    Ok(BuiltSpec { spec, summary })
}

/// Read every blob the genesis names, from disk or over http.
async fn fetch_assets(
    genesis: &JamGenesisConfig,
) -> Result<HashMap<AssetLocation, Vec<u8>>, GeneratorError> {
    let mut assets = HashMap::new();
    for location in genesis.asset_locations() {
        if assets.contains_key(location) {
            continue;
        }
        let content = location.get_asset().await.map_err(|e| {
            GeneratorError::ChainSpecGeneration(format!("JAM genesis asset {location}: {e}"))
        })?;
        assets.insert(location.clone(), content);
    }
    Ok(assets)
}

fn protocol_parameters(
    preset: &JamProtocolParameterType,
    genesis: &JamGenesisConfig,
) -> Result<ProtocolParameters, GeneratorError> {
    if let Some(value) = genesis.protocol_parameters() {
        return serde_json::from_value(value.clone()).map_err(|e| {
            GeneratorError::ChainSpecGeneration(format!(
                "JAM genesis protocol_parameters must set every parameter: {e}"
            ))
        });
    }
    Ok(match preset {
        JamProtocolParameterType::Tiny => ProtocolParameters::tiny(),
        JamProtocolParameterType::Mini => ProtocolParameters::mini(),
        JamProtocolParameterType::Full => ProtocolParameters::full(),
    })
}

/// `jam-chainspec` needs exactly as many validators as the parameters call for.
fn check_validator_count(
    preset: &JamProtocolParameterType,
    genesis: &JamGenesisConfig,
    parameters: &ProtocolParameters,
    validators: &[GenesisValidator],
) -> Result<(), GeneratorError> {
    let expected = parameters.core_count as usize * VALS_PER_CORE;
    let got = validators.len();
    if got == expected {
        return Ok(());
    }
    let parameters_name = if genesis.protocol_parameters().is_some() {
        "the given protocol parameters".to_string()
    } else {
        format!("`{}`", preset.as_str())
    };
    Err(GeneratorError::ChainSpecGeneration(format!(
        "the built-in JAM spec builder needs exactly {expected} validators for \
         {parameters_name}, got {got}. Add validators, set `genesis.protocol_parameters`, or \
         give a pre-built spec with `chain_spec_path`."
    )))
}

/// The config file's chain (if any), with zombienet's id and validators.
fn base_config(
    id: &str,
    genesis: &JamGenesisConfig,
    validators: &[GenesisValidator],
    assets: &HashMap<AssetLocation, Vec<u8>>,
) -> Result<ChainSpecConfig, GeneratorError> {
    let (mut base, base_dir) = match genesis.config_file() {
        Some(location) => {
            let base: serde_json::Value =
                serde_json::from_slice(&assets[location]).map_err(|e| {
                    GeneratorError::ChainSpecGeneration(format!(
                        "JAM genesis config_file {location}: {e}"
                    ))
                })?;
            if !base.is_object() {
                return Err(GeneratorError::ChainSpecGeneration(format!(
                    "JAM genesis config_file {location}: must be a JSON object"
                )));
            }
            // Blob paths in the file are relative to it.
            let base_dir = match location {
                AssetLocation::FilePath(path) => path.parent().map(Path::to_path_buf),
                AssetLocation::Url(_) => None,
            };
            (base, base_dir)
        },
        None => (json!({}), None),
    };

    base["id"] = json!(id);
    base["genesis_validators"] = serde_json::to_value(validators)
        .map_err(|e| GeneratorError::EncodeDecodeError(e.to_string()))?;
    // Set as a value afterwards, not as JSON.
    if let Some(object) = base.as_object_mut() {
        object.remove("protocol_parameters");
    }

    ChainSpecConfig::from_json(&base.to_string(), base_dir.as_deref()).map_err(spec_error)
}

/// Lay the typed genesis over `config`. Services and auth queues replace whole entries.
fn apply_genesis(
    mut config: ChainSpecConfig,
    genesis: &JamGenesisConfig,
    assets: &HashMap<AssetLocation, Vec<u8>>,
) -> ChainSpecConfig {
    for (id, service) in genesis.services() {
        let mut genesis_service = GenesisService::new(Blob::Bytes(assets[service.code()].clone()))
            .balance(match service.balance() {
                JamBalance::Exact(balance) => GenesisBalance::Exact(balance),
                JamBalance::MinimumPlus(extra) => GenesisBalance::MinimumPlus(extra),
            })
            .min_item_gas(service.min_item_gas())
            .min_memo_gas(service.min_memo_gas())
            .deposit_offset(service.deposit_offset());
        for preimage in service.preimages() {
            genesis_service = genesis_service.preimage(Blob::Bytes(assets[preimage].clone()));
        }
        for (key, value) in service.storage() {
            genesis_service = genesis_service.storage(key.0.clone(), value.0.clone());
        }
        config.services.insert(*id, genesis_service);
    }

    for (core, authorizers) in genesis.auth_queues() {
        let hashes: Vec<AuthorizerHash> = authorizers
            .iter()
            .map(|authorizer| match authorizer {
                JamAuthorizer::Null => jam_blobs::null_authorizer_hash(),
                JamAuthorizer::Hash(hash) => (*hash).into(),
                JamAuthorizer::Code { code, config } => {
                    jam_blobs::authorizer_hash(&assets[code], config.as_bytes())
                },
            })
            .collect();
        config = config.auth_queue(*core, hashes);
    }

    // `0` is the default holder of each privilege, so it doesn't replace the config file's.
    let privileges = genesis.privileges();
    if privileges.manager() != 0 {
        config = config.manager(privileges.manager());
    }
    if privileges.delegator() != 0 {
        config = config.delegator(privileges.delegator());
    }
    if privileges.registrar() != 0 {
        config = config.registrar(privileges.registrar());
    }
    for (core, id) in privileges.assign() {
        config = config.assigner([*core], *id);
    }
    for (id, gas) in privileges.always_acc() {
        config = config.always_acc(*id, *gas);
    }

    config
}

/// Add the bootstrap service and the null authorizer queues, leaving whatever the config already
/// sets in place.
fn with_bootstrap_defaults(
    mut config: ChainSpecConfig,
    bootstrap_code: Vec<u8>,
    core_count: u16,
) -> ChainSpecConfig {
    config
        .services
        .entry(JAM_BOOTSTRAP_SERVICE_ID)
        .or_insert_with(|| {
            // The same account the node's `gen-spec` creates.
            GenesisService::new(bootstrap_code)
                .preimage(NULL_AUTHORIZER_BLOB)
                .min_item_gas(10)
                .min_memo_gas(10)
                .deposit_offset(Balance::MAX)
                .balance(Balance::MAX)
        });

    let null_authorizer = jam_blobs::null_authorizer_hash();
    for core in 0..core_count {
        config
            .auth_queues
            .entry(core)
            .or_insert_with(|| vec![null_authorizer.into()]);
    }
    config
}

/// Compile the config, one spec at a time, and always with the same parameters.
fn build_spec(config: ChainSpecConfig) -> Result<String, GeneratorError> {
    let parameters = config
        .protocol_parameters
        .expect("the parameters are always set before building; qed");
    let _guard = BUILD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let applied = APPLIED_PARAMETERS.get_or_init(|| parameters);
    if *applied != parameters {
        return Err(GeneratorError::ChainSpecGeneration(
            "the built-in JAM spec builder supports a single set of protocol parameters per \
             process, and another set was already used. Spawn the networks from separate \
             processes, or give a pre-built spec with `chain_spec_path`."
                .to_string(),
        ));
    }

    let spec = ChainSpec::from_config_and_apply_parameters(&config).map_err(spec_error)?;
    Ok(spec.to_json())
}

/// The bootstrap service installed with the defaults.
struct BootstrapService {
    code: Vec<u8>,
    /// Where it comes from, for the logs and the summary.
    source: String,
}

/// The genesis' own bootstrap service, else the embedded one.
fn bootstrap_service(
    genesis: &JamGenesisConfig,
    assets: &HashMap<AssetLocation, Vec<u8>>,
) -> Result<BootstrapService, GeneratorError> {
    if let Some(location) = genesis.bootstrap_service() {
        return Ok(BootstrapService {
            code: assets[location].clone(),
            source: location.to_string(),
        });
    }
    match jam_blobs::embedded_bootstrap_service() {
        Some((code, source)) => Ok(BootstrapService {
            code: code.to_vec(),
            source: format!("embedded, {source}"),
        }),
        None => Err(GeneratorError::ChainSpecGeneration(
            "no JAM bootstrap service: zombienet was built without the `jam` feature, which \
             embeds one. Set `genesis.bootstrap_service` to a blob (a path or a url), enable the \
             `jam` feature, or give a pre-built spec with `chain_spec_path`."
                .to_string(),
        )),
    }
}

fn summary(
    genesis: &JamGenesisConfig,
    config: &ChainSpecConfig,
    bootstrap: Option<&BootstrapService>,
    parameters: &ProtocolParameters,
) -> serde_json::Value {
    let services: serde_json::Map<String, serde_json::Value> = config
        .services
        .iter()
        .map(|(id, service)| {
            let code_hash = service
                .code
                .read()
                .map(|code| format!("0x{}", hex::to_hex(&hash_raw(&code))))
                .unwrap_or_default();
            (id.to_string(), json!({ "code_hash": code_hash }))
        })
        .collect();

    json!({
        "bootstrap_service": bootstrap.map(|bootstrap| json!({
            "source": bootstrap.source,
            "code_hash": format!("0x{}", hex::to_hex(&hash_raw(&bootstrap.code))),
        })),
        "protocol_parameters": parameters,
        "services": services,
        "genesis": genesis,
    })
}

fn spec_error(error: jam_chainspec::Error) -> GeneratorError {
    GeneratorError::ChainSpecGeneration(error.to_string())
}

#[cfg(test)]
mod tests {
    use configuration::{JamGenesisConfigBuilder, JamchainConfigBuilder};

    use super::*;

    const SERVICE_CODE: &[u8] = b"my service code";
    const AUTHORIZER_CODE: &[u8] = b"my authorizer code";

    /// A tiny chain with `validators` validators and `genesis`.
    fn jam_spec(
        validators: usize,
        genesis: impl FnOnce(JamGenesisConfigBuilder) -> JamGenesisConfigBuilder,
    ) -> (JamchainSpec, JamGenesisConfig) {
        let mut chain = JamchainConfigBuilder::new(Default::default())
            .with_id("dev")
            .with_genesis(genesis)
            .with_validator(|n| n.with_name("jam0"));
        for i in 1..validators {
            chain = chain.with_validator(|n| n.with_name(&format!("jam{i}")));
        }
        let config = chain.build().expect("the chain config builds");
        let genesis = config.genesis().cloned().unwrap_or_default();
        let spec = JamchainSpec::from_config(&config).expect("the spec builds");
        (spec, genesis)
    }

    fn state_values(spec: &str) -> Vec<String> {
        let spec: serde_json::Value = serde_json::from_str(spec).unwrap();
        spec["genesis_state"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn contains_blob(values: &[String], blob: &[u8]) -> bool {
        values.contains(&hex::to_hex(blob).to_string())
    }

    fn write(dir: &Path, name: &str, content: &[u8]) -> String {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    // Everything building a spec runs in one test: the ring parameters are sized once per
    // process.
    #[tokio::test]
    async fn specs_are_built_from_the_genesis() {
        // The default genesis: the embedded bootstrap service and the null authorizer.
        #[cfg(feature = "jam")]
        {
            let (spec, genesis) = jam_spec(6, |g| g);
            let built = build(&spec, &genesis).await.expect("the spec builds");
            let parsed: serde_json::Value = serde_json::from_str(&built.spec).unwrap();
            assert_eq!(parsed["id"], "dev");
            let values = state_values(&built.spec);
            assert!(contains_blob(&values, jam_blobs::BOOTSTRAP_SERVICE_BLOB));
            assert!(contains_blob(&values, NULL_AUTHORIZER_BLOB));
            assert_eq!(
                built.summary["bootstrap_service"]["source"],
                format!("embedded, {}", jam_blobs::BOOTSTRAP_SERVICE_SOURCE)
            );
            assert_eq!(
                built.summary["bootstrap_service"]["code_hash"],
                format!("0x{}", hex::to_hex(&jam_blobs::BOOTSTRAP_SERVICE_HASH))
            );
        }
        // Without the `jam` feature there is none to embed, so the genesis must name one.
        #[cfg(not(feature = "jam"))]
        {
            let (spec, genesis) = jam_spec(6, |g| g);
            let error = build(&spec, &genesis).await.err().unwrap();
            assert!(error.to_string().contains("`jam` feature"), "{error}");

            // Without the defaults, none is needed.
            let (spec, genesis) = jam_spec(6, |g| g.without_defaults());
            let built = build(&spec, &genesis).await.expect("the spec builds");
            assert!(built.summary["bootstrap_service"].is_null());
        }

        // A bootstrap service of the user's own, a service and an authorizer from files.
        let dir = tempfile::tempdir().unwrap();
        let bootstrap = write(dir.path(), "bootstrap.jam", b"own bootstrap code");
        let service = write(dir.path(), "svc.jam", SERVICE_CODE);
        let authorizer = write(dir.path(), "auth.jam", AUTHORIZER_CODE);
        let (spec, genesis) = jam_spec(6, |g| {
            g.with_bootstrap_service(bootstrap.as_str())
                .with_service(5, |s| {
                    s.with_code(service.as_str())
                        .with_balance(JamBalance::MinimumPlus(1_000))
                        .with_preimage(authorizer.as_str())
                        .with_storage(b"greeting".to_vec(), b"hello".to_vec())
                })
                .with_auth_queue(0, [JamAuthorizer::code(authorizer.as_str())])
                .with_assigner([0], 5)
        });
        let built = build(&spec, &genesis).await.expect("the spec builds");
        let values = state_values(&built.spec);
        assert!(contains_blob(&values, b"own bootstrap code"));
        assert!(contains_blob(&values, NULL_AUTHORIZER_BLOB));
        #[cfg(feature = "jam")]
        assert!(!contains_blob(&values, jam_blobs::BOOTSTRAP_SERVICE_BLOB));
        assert!(contains_blob(&values, SERVICE_CODE));
        assert!(contains_blob(&values, AUTHORIZER_CODE));
        assert!(contains_blob(&values, b"hello"));
        assert_eq!(
            built.summary["bootstrap_service"]["source"],
            bootstrap.as_str()
        );
        assert!(built.summary["services"]["5"].is_object());

        // A native config file, with the typed genesis on top.
        write(dir.path(), "from-file.jam", b"service from the file");
        let config_file = write(
            dir.path(),
            "chainspec.json",
            br#"{
                "id": "ignored",
                "genesis_validators": [],
                "services": { "7": { "code": "from-file.jam", "balance": 1000000 } }
            }"#,
        );
        let (spec, genesis) = jam_spec(6, |g| {
            g.with_bootstrap_service(bootstrap.as_str())
                .with_config_file(config_file.as_str())
                .with_service(5, |s| s.with_code(service.as_str()))
        });
        let built = build(&spec, &genesis).await.expect("the spec builds");
        let values = state_values(&built.spec);
        assert!(contains_blob(&values, b"service from the file"));
        assert!(contains_blob(&values, SERVICE_CODE));
        assert!(contains_blob(&values, b"own bootstrap code"));
        let parsed: serde_json::Value = serde_json::from_str(&built.spec).unwrap();
        assert_eq!(parsed["id"], "dev");

        // The genesis is checked, not dropped: a queue for a core the chain lacks is refused.
        let (spec, genesis) = jam_spec(6, |g| {
            g.with_bootstrap_service(bootstrap.as_str())
                .with_auth_queue(7, [JamAuthorizer::Null])
        });
        let error = build(&spec, &genesis).await.err().unwrap();
        assert!(error.to_string().contains("core 7"), "{error}");

        // Another set of parameters can't be used by the same process.
        let mini = serde_json::to_value(ProtocolParameters::mini()).unwrap();
        let (spec, genesis) = jam_spec(78, |g| {
            g.with_bootstrap_service(bootstrap.as_str())
                .with_protocol_parameters(mini)
        });
        let error = build(&spec, &genesis).await.err().unwrap();
        assert!(
            error
                .to_string()
                .contains("single set of protocol parameters"),
            "{error}"
        );
    }

    /// With the defaults, the genesis is the one the node's own `gen-spec` writes. The node's
    /// bootstrap service is a build of its own (the embedded one is built from the
    /// `jam-bootstrap-service` crate, and builds aren't reproducible across toolchains), so it is
    /// taken from the node's spec and given as `bootstrap_service`. Run with `POLKAJAM_BIN`
    /// pointing at a `polkajam` of the version the `jam-*` crates come from.
    ///
    /// Two system keys are written differently by `jam-chainspec` 0.1.29 and the node, and are
    /// left out: the first epoch's fallback sealing keys (round-robin vs the node's order) and
    /// the activity statistics (empty vs one zeroed entry per validator). The node runs on
    /// either, as the `jam-smoke-*` tests show.
    #[tokio::test]
    #[ignore = "needs POLKAJAM_BIN"]
    async fn the_default_genesis_matches_gen_spec() {
        let polkajam = std::env::var("POLKAJAM_BIN").expect("POLKAJAM_BIN is set");
        let dir = tempfile::tempdir().unwrap();

        // The node's spec, for the same validators.
        let (spec, _) = jam_spec(6, |g| g);
        let config_path = dir.path().join("jam_config.json");
        let spec_path = dir.path().join("jam_spec.json");
        std::fs::write(
            &config_path,
            json!({
                "id": spec.id.as_str(),
                "genesis_validators": jam_validators::genesis_validators(&spec),
            })
            .to_string(),
        )
        .unwrap();
        let status = std::process::Command::new(polkajam)
            .arg("gen-spec")
            .arg(&config_path)
            .arg(&spec_path)
            .status()
            .unwrap();
        assert!(status.success());
        let theirs: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&spec_path).unwrap()).unwrap();

        // Its bootstrap service: the largest value of the state.
        let bootstrap = theirs["genesis_state"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| hex::from_hex(v.as_str().unwrap()).unwrap())
            .max_by_key(Vec::len)
            .unwrap();
        let bootstrap_path = write(dir.path(), "bootstrap.jam", &bootstrap);

        // The same nodes (their ports are in the validator set), with the node's bootstrap.
        let genesis = JamGenesisConfigBuilder::new()
            .with_bootstrap_service(bootstrap_path.as_str())
            .build()
            .unwrap();
        let built = build(&spec, &genesis).await.expect("the spec builds");
        let ours: serde_json::Value = serde_json::from_str(&built.spec).unwrap();

        for key in ["id", "genesis_header", "protocol_parameters"] {
            assert_eq!(ours[key], theirs[key], "{key} differs");
        }
        const SAFROLE: &str = "04000000000000000000000000000000000000000000000000000000000000";
        const STATISTICS: &str = "0d000000000000000000000000000000000000000000000000000000000000";
        let state = |spec: &serde_json::Value| {
            let mut state = spec["genesis_state"].as_object().unwrap().clone();
            state.remove(SAFROLE);
            state.remove(STATISTICS);
            state
        };
        let (ours, theirs) = (state(&ours), state(&theirs));
        let differing: Vec<&String> = ours
            .keys()
            .chain(theirs.keys())
            .filter(|key| ours.get(*key) != theirs.get(*key))
            .collect();
        assert!(
            differing.is_empty(),
            "genesis_state differs at {differing:?}"
        );
    }

    #[tokio::test]
    async fn the_validator_count_must_match_the_parameters() {
        let (spec, genesis) = jam_spec(4, |g| g);

        let error = build(&spec, &genesis).await.err().unwrap();

        assert!(
            error
                .to_string()
                .contains("needs exactly 6 validators for `tiny`, got 4"),
            "{error}"
        );
        assert!(error.to_string().contains("chain_spec_path"), "{error}");
    }

    #[tokio::test]
    async fn a_missing_asset_is_reported() {
        let (spec, genesis) = jam_spec(6, |g| g.with_bootstrap_service("/does/not/exist.jam"));

        let error = build(&spec, &genesis).await.err().unwrap();

        assert!(error.to_string().contains("/does/not/exist.jam"), "{error}");
    }

    #[test]
    fn every_core_gets_the_null_authorizer_unless_its_queue_is_set() {
        let mut config = ChainSpecConfig::new("dev", vec![]).unwrap();
        let own = AuthorizerHash::from([7; 32]);
        config.auth_queues.insert(1, vec![own.into()]);

        let config = with_bootstrap_defaults(config, b"bootstrap".to_vec(), 2);

        assert_eq!(
            config.auth_queues[&0][0].0,
            jam_blobs::null_authorizer_hash()
        );
        assert_eq!(config.auth_queues[&1][0].0, own);
        assert!(config.services.contains_key(&JAM_BOOTSTRAP_SERVICE_ID));
    }

    #[test]
    fn a_bootstrap_service_from_the_genesis_is_kept() {
        let mut config = ChainSpecConfig::new("dev", vec![]).unwrap();
        config.services.insert(
            JAM_BOOTSTRAP_SERVICE_ID,
            GenesisService::new(b"own bootstrap").balance(1),
        );

        let config = with_bootstrap_defaults(config, b"bootstrap".to_vec(), 2);

        assert_eq!(
            config.services[&JAM_BOOTSTRAP_SERVICE_ID]
                .code
                .read()
                .unwrap(),
            b"own bootstrap"
        );
    }
}
