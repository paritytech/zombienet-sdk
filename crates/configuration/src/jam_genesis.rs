//! The genesis of a JAM chain built by zombienet itself (with `jam-chainspec`).
//!
//! This is plain data: the orchestrator turns it into a `jam_chainspec::ChainSpecConfig`. The
//! chain id, the validator set and the protocol parameter preset always come from the
//! [`crate::JamchainConfig`], everything else (services, auth queues, privileges) from here.

use std::{
    collections::BTreeMap,
    fmt::{self, Display},
    path::Path,
    str::FromStr,
};

use anyhow::anyhow;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::shared::types::AssetLocation;

/// Id of a JAM service.
pub type JamServiceId = u32;
/// Index of a JAM core.
pub type JamCoreIndex = u16;

/// The id the bootstrap service is installed under.
pub const JAM_BOOTSTRAP_SERVICE_ID: JamServiceId = 0;

/// The genesis of a JAM chain built in process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JamGenesisConfig {
    /// Add the bootstrap service (id 0, hosting the null authorizer) and the null authorizer on
    /// every core whose queue is not set. Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    with_defaults: bool,
    /// Code of the bootstrap service. `None` uses the blob embedded in zombienet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bootstrap_service: Option<AssetLocation>,
    /// A native `jam-chainspec` JSON config, layered under the fields below. Its `id`,
    /// `genesis_validators` and `protocol_parameters` are replaced by zombienet's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_file: Option<AssetLocation>,
    /// Full protocol parameters object, overriding the `protocol_params_type` preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol_parameters: Option<serde_json::Value>,
    /// Services to install at genesis, by id.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "string_keyed"
    )]
    services: BTreeMap<JamServiceId, JamGenesisService>,
    /// Per-core authorization queues.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "string_keyed"
    )]
    auth_queues: BTreeMap<JamCoreIndex, Vec<JamAuthorizer>>,
    #[serde(default, skip_serializing_if = "JamGenesisPrivileges::is_empty")]
    privileges: JamGenesisPrivileges,
}

impl Default for JamGenesisConfig {
    fn default() -> Self {
        Self {
            with_defaults: true,
            bootstrap_service: None,
            config_file: None,
            protocol_parameters: None,
            services: BTreeMap::new(),
            auth_queues: BTreeMap::new(),
            privileges: JamGenesisPrivileges::default(),
        }
    }
}

impl JamGenesisConfig {
    /// Whether the bootstrap service and the null authorizer queues are added.
    pub fn with_defaults(&self) -> bool {
        self.with_defaults
    }

    /// The bootstrap service code, `None` for the embedded one.
    pub fn bootstrap_service(&self) -> Option<&AssetLocation> {
        self.bootstrap_service.as_ref()
    }

    /// The native `jam-chainspec` config layered under this one.
    pub fn config_file(&self) -> Option<&AssetLocation> {
        self.config_file.as_ref()
    }

    /// The full protocol parameters object, if set.
    pub fn protocol_parameters(&self) -> Option<&serde_json::Value> {
        self.protocol_parameters.as_ref()
    }

    /// The services installed at genesis.
    pub fn services(&self) -> &BTreeMap<JamServiceId, JamGenesisService> {
        &self.services
    }

    /// The per-core authorization queues.
    pub fn auth_queues(&self) -> &BTreeMap<JamCoreIndex, Vec<JamAuthorizer>> {
        &self.auth_queues
    }

    /// The privileged services.
    pub fn privileges(&self) -> &JamGenesisPrivileges {
        &self.privileges
    }

    /// Every asset this genesis refers to.
    pub fn asset_locations(&self) -> Vec<&AssetLocation> {
        let mut locations: Vec<&AssetLocation> = self
            .bootstrap_service
            .iter()
            .chain(self.config_file.iter())
            .collect();
        for service in self.services.values() {
            locations.push(&service.code);
            locations.extend(service.preimages.iter());
        }
        for authorizer in self.auth_queues.values().flatten() {
            if let JamAuthorizer::Code { code, .. } = authorizer {
                locations.push(code);
            }
        }
        locations
    }

    /// Make relative file paths absolute, against the directory of the network definition.
    pub(crate) fn resolve_relative_paths(&mut self, base_dir: &Path) {
        let resolve = |location: &mut AssetLocation| {
            if let AssetLocation::FilePath(path) = location {
                if path.is_relative() {
                    *path = base_dir.join(&*path);
                }
            }
        };
        self.bootstrap_service.iter_mut().for_each(resolve);
        self.config_file.iter_mut().for_each(resolve);
        for service in self.services.values_mut() {
            resolve(&mut service.code);
            service.preimages.iter_mut().for_each(resolve);
        }
        for authorizer in self.auth_queues.values_mut().flatten() {
            if let JamAuthorizer::Code { code, .. } = authorizer {
                resolve(code);
            }
        }
    }
}

/// A service as it exists at genesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JamGenesisService {
    /// The service code.
    code: AssetLocation,
    /// The starting balance.
    #[serde(default, skip_serializing_if = "JamBalance::is_minimum")]
    balance: JamBalance,
    /// Minimum gas per accumulated work item.
    #[serde(default, skip_serializing_if = "is_zero")]
    min_item_gas: u64,
    /// Minimum gas per incoming transfer.
    #[serde(default, skip_serializing_if = "is_zero")]
    min_memo_gas: u64,
    /// Footprint below which no deposit is owed.
    #[serde(default, skip_serializing_if = "JamAmount::is_zero")]
    deposit_offset: JamAmount,
    /// Further preimages hosted by the service, available from slot 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    preimages: Vec<AssetLocation>,
    /// Storage entries, hex keys to hex values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    storage: BTreeMap<JamBytes, JamBytes>,
}

impl JamGenesisService {
    pub fn code(&self) -> &AssetLocation {
        &self.code
    }

    pub fn balance(&self) -> JamBalance {
        self.balance
    }

    pub fn min_item_gas(&self) -> u64 {
        self.min_item_gas
    }

    pub fn min_memo_gas(&self) -> u64 {
        self.min_memo_gas
    }

    pub fn deposit_offset(&self) -> u64 {
        self.deposit_offset.0
    }

    pub fn preimages(&self) -> &[AssetLocation] {
        &self.preimages
    }

    pub fn storage(&self) -> &BTreeMap<JamBytes, JamBytes> {
        &self.storage
    }
}

/// Who holds the privileges at genesis. `0` is the bootstrap service.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JamGenesisPrivileges {
    /// The service with the power to bless (the manager).
    #[serde(alias = "bless", skip_serializing_if = "is_zero_u32")]
    manager: JamServiceId,
    /// The service with the power to designate (the delegator).
    #[serde(alias = "designate", skip_serializing_if = "is_zero_u32")]
    delegator: JamServiceId,
    /// The service with the power to register services (the registrar).
    #[serde(alias = "register", skip_serializing_if = "is_zero_u32")]
    registrar: JamServiceId,
    /// The service that may assign authorizers to each core.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", with = "string_keyed")]
    assign: BTreeMap<JamCoreIndex, JamServiceId>,
    /// The services which always accumulate, with their free gas.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", with = "string_keyed")]
    always_acc: BTreeMap<JamServiceId, u64>,
}

impl JamGenesisPrivileges {
    pub fn manager(&self) -> JamServiceId {
        self.manager
    }

    pub fn delegator(&self) -> JamServiceId {
        self.delegator
    }

    pub fn registrar(&self) -> JamServiceId {
        self.registrar
    }

    pub fn assign(&self) -> &BTreeMap<JamCoreIndex, JamServiceId> {
        &self.assign
    }

    pub fn always_acc(&self) -> &BTreeMap<JamServiceId, u64> {
        &self.always_acc
    }

    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// The starting balance of a service.
///
/// TOML integers are signed, so the largest balances can't be written as numbers. Accepted
/// forms: `1000`, `"1000"`, `"max"`, `"min"` (just the deposit) and `"min+1000"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JamBalance {
    /// Exactly this balance; it must cover the deposit the service's footprint implies.
    Exact(u64),
    /// The deposit the service's footprint implies, plus this.
    MinimumPlus(u64),
}

impl Default for JamBalance {
    fn default() -> Self {
        Self::MinimumPlus(0)
    }
}

impl JamBalance {
    fn is_minimum(&self) -> bool {
        self == &Self::MinimumPlus(0)
    }
}

impl From<u64> for JamBalance {
    fn from(balance: u64) -> Self {
        Self::Exact(balance)
    }
}

impl FromStr for JamBalance {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "min" {
            return Ok(Self::MinimumPlus(0));
        }
        if let Some(extra) = s.strip_prefix("min+") {
            return Ok(Self::MinimumPlus(JamAmount::from_str(extra.trim())?.0));
        }
        Ok(Self::Exact(JamAmount::from_str(s)?.0))
    }
}

impl Display for JamBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(balance) => write!(f, "{}", JamAmount(*balance)),
            Self::MinimumPlus(0) => write!(f, "min"),
            Self::MinimumPlus(extra) => write!(f, "min+{}", JamAmount(*extra)),
        }
    }
}

impl Serialize for JamBalance {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Exact(balance) => JamAmount(*balance).serialize(serializer),
            other => serializer.serialize_str(&other.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for JamBalance {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NumberOrStrVisitor::<Self>::new(
            "a balance: an integer, \"max\", \"min\" or \"min+<amount>\"",
        ))
    }
}

/// An unsigned amount that also accepts `"max"` and numeric strings, so values above `i64::MAX`
/// can be written in TOML.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JamAmount(pub u64);

impl JamAmount {
    fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl FromStr for JamAmount {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "max" {
            return Ok(Self(u64::MAX));
        }
        s.replace('_', "")
            .parse()
            .map(Self)
            .map_err(|e| anyhow!("'{s}' is not an amount: {e}"))
    }
}

impl From<u64> for JamAmount {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Display for JamAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == u64::MAX {
            write!(f, "max")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl Serialize for JamAmount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Anything a TOML integer can't hold goes as a string.
        if self.0 <= i64::MAX as u64 {
            serializer.serialize_u64(self.0)
        } else {
            serializer.serialize_str(&self.to_string())
        }
    }
}

impl<'de> Deserialize<'de> for JamAmount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NumberOrStrVisitor::<Self>::new(
            "an amount: an integer or \"max\"",
        ))
    }
}

/// Deserializes an unsigned integer, or a string parsed with `FromStr`.
struct NumberOrStrVisitor<T> {
    expecting: &'static str,
    _marker: std::marker::PhantomData<T>,
}

impl<T> NumberOrStrVisitor<T> {
    fn new(expecting: &'static str) -> Self {
        Self {
            expecting,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> de::Visitor<'_> for NumberOrStrVisitor<T>
where
    T: From<u64> + FromStr<Err = anyhow::Error>,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<T, E> {
        Ok(T::from(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<T, E> {
        u64::try_from(v)
            .map(T::from)
            .map_err(|_| E::custom(format!("{v} is negative")))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        T::from_str(v).map_err(E::custom)
    }
}

/// Bytes, written as a `0x`-prefixed hex string.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JamBytes(pub Vec<u8>);

impl JamBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for JamBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for JamBytes {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl<const N: usize> From<&[u8; N]> for JamBytes {
    fn from(bytes: &[u8; N]) -> Self {
        Self(bytes.to_vec())
    }
}

impl<const N: usize> From<[u8; N]> for JamBytes {
    fn from(bytes: [u8; N]) -> Self {
        Self(bytes.to_vec())
    }
}

impl FromStr for JamBytes {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(digits)
            .map(Self)
            .map_err(|e| anyhow!("'{s}' is not hex: {e}"))
    }
}

impl Display for JamBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}

impl Serialize for JamBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for JamBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

/// An entry of a core's authorization queue.
///
/// Written as `"null"` (the null authorizer), a `0x`-prefixed 32-byte authorizer hash, or
/// `{ code = "<path or url>", config = "0x.." }`, which zombienet hashes into the authorizer
/// hash.
#[derive(Debug, Clone, PartialEq)]
pub enum JamAuthorizer {
    /// The null authorizer, which always authorizes.
    Null,
    /// An authorizer hash, as is.
    Hash([u8; 32]),
    /// The authorizer with this code and configuration.
    Code {
        code: AssetLocation,
        config: JamBytes,
    },
}

impl JamAuthorizer {
    /// The parameterless authorizer with `code`.
    pub fn code(code: impl Into<AssetLocation>) -> Self {
        Self::Code {
            code: code.into(),
            config: JamBytes::default(),
        }
    }

    /// The authorizer with `code` and `config`.
    pub fn code_with_config(code: impl Into<AssetLocation>, config: impl Into<Vec<u8>>) -> Self {
        Self::Code {
            code: code.into(),
            config: JamBytes(config.into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum JamAuthorizerRepr {
    Text(String),
    Code {
        code: AssetLocation,
        #[serde(default, skip_serializing_if = "is_empty_bytes")]
        config: JamBytes,
    },
}

impl Serialize for JamAuthorizer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => JamAuthorizerRepr::Text("null".into()),
            Self::Hash(hash) => JamAuthorizerRepr::Text(JamBytes(hash.to_vec()).to_string()),
            Self::Code { code, config } => JamAuthorizerRepr::Code {
                code: code.clone(),
                config: config.clone(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JamAuthorizer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match JamAuthorizerRepr::deserialize(deserializer).map_err(|_| {
            de::Error::custom(
                "an authorizer: \"null\", a 0x-prefixed 32-byte hash, or { code = \"..\", config = \"0x..\" }",
            )
        })? {
            JamAuthorizerRepr::Text(text) if text == "null" => Ok(Self::Null),
            JamAuthorizerRepr::Text(text) => {
                let bytes = JamBytes::from_str(&text).map_err(de::Error::custom)?;
                let hash: [u8; 32] = bytes.0.try_into().map_err(|_| {
                    de::Error::custom(format!("authorizer hash '{text}' is not 32 bytes"))
                })?;
                Ok(Self::Hash(hash))
            },
            JamAuthorizerRepr::Code { code, config } => Ok(Self::Code { code, config }),
        }
    }
}

/// Builder for [`JamGenesisConfig`].
pub struct JamGenesisConfigBuilder {
    config: JamGenesisConfig,
    errors: Vec<anyhow::Error>,
}

impl Default for JamGenesisConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl JamGenesisConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: JamGenesisConfig::default(),
            errors: vec![],
        }
    }

    /// Continue building from `config`.
    pub(crate) fn from_config(config: JamGenesisConfig) -> Self {
        Self {
            config,
            errors: vec![],
        }
    }

    /// Use this bootstrap service code (a path or a url) instead of the embedded one.
    pub fn with_bootstrap_service(mut self, location: impl Into<AssetLocation>) -> Self {
        self.config.bootstrap_service = Some(location.into());
        self
    }

    /// Don't add the bootstrap service nor the null authorizer queues.
    pub fn without_defaults(mut self) -> Self {
        self.config.with_defaults = false;
        self
    }

    /// Layer the genesis over this native `jam-chainspec` JSON config.
    pub fn with_config_file(mut self, location: impl Into<AssetLocation>) -> Self {
        self.config.config_file = Some(location.into());
        self
    }

    /// Use these protocol parameters (a full parameters object) instead of the preset.
    pub fn with_protocol_parameters(mut self, parameters: impl Into<serde_json::Value>) -> Self {
        let parameters = parameters.into();
        if parameters.is_object() {
            self.config.protocol_parameters = Some(parameters);
        } else {
            self.errors.push(anyhow!(
                "protocol_parameters: must be an object with every parameter"
            ));
        }
        self
    }

    /// Install a service under `id`, using a nested [`JamGenesisServiceBuilder`].
    pub fn with_service(
        mut self,
        id: JamServiceId,
        f: impl FnOnce(JamGenesisServiceBuilder) -> JamGenesisServiceBuilder,
    ) -> Self {
        match f(JamGenesisServiceBuilder::new()).build() {
            Ok(service) => {
                self.config.services.insert(id, service);
            },
            Err(errors) => self.errors.extend(
                errors
                    .into_iter()
                    .map(|error| anyhow!("services[{id}].{error}")),
            ),
        }
        self
    }

    /// Set the authorization queue of `core`.
    pub fn with_auth_queue(
        mut self,
        core: JamCoreIndex,
        authorizers: impl IntoIterator<Item = JamAuthorizer>,
    ) -> Self {
        self.config
            .auth_queues
            .insert(core, authorizers.into_iter().collect());
        self
    }

    /// Let the service `id` assign authorizers to each of `cores`.
    pub fn with_assigner(
        mut self,
        cores: impl IntoIterator<Item = JamCoreIndex>,
        id: JamServiceId,
    ) -> Self {
        for core in cores {
            self.config.privileges.assign.insert(core, id);
        }
        self
    }

    /// Set the manager service (the `bless` privilege).
    pub fn with_manager(mut self, id: JamServiceId) -> Self {
        self.config.privileges.manager = id;
        self
    }

    /// Set the delegator service (the `designate` privilege).
    pub fn with_delegator(mut self, id: JamServiceId) -> Self {
        self.config.privileges.delegator = id;
        self
    }

    /// Set the registrar service (the `register` privilege).
    pub fn with_registrar(mut self, id: JamServiceId) -> Self {
        self.config.privileges.registrar = id;
        self
    }

    /// Let the service `id` always accumulate, with `gas` free gas per block.
    pub fn with_always_acc(mut self, id: JamServiceId, gas: u64) -> Self {
        self.config.privileges.always_acc.insert(id, gas);
        self
    }

    pub fn build(self) -> Result<JamGenesisConfig, Vec<anyhow::Error>> {
        if self.errors.is_empty() {
            Ok(self.config)
        } else {
            Err(self.errors)
        }
    }
}

/// Builder for [`JamGenesisService`].
pub struct JamGenesisServiceBuilder {
    code: Option<AssetLocation>,
    service: JamGenesisService,
}

impl JamGenesisServiceBuilder {
    fn new() -> Self {
        Self {
            code: None,
            service: JamGenesisService {
                code: AssetLocation::FilePath(Default::default()),
                balance: JamBalance::default(),
                min_item_gas: 0,
                min_memo_gas: 0,
                deposit_offset: JamAmount::default(),
                preimages: vec![],
                storage: BTreeMap::new(),
            },
        }
    }

    /// The service code (a path or a url). Required.
    pub fn with_code(mut self, location: impl Into<AssetLocation>) -> Self {
        self.code = Some(location.into());
        self
    }

    /// The starting balance. Default: just enough to cover the deposit.
    pub fn with_balance(mut self, balance: impl Into<JamBalance>) -> Self {
        self.service.balance = balance.into();
        self
    }

    /// Minimum gas per accumulated work item.
    pub fn with_min_item_gas(mut self, gas: u64) -> Self {
        self.service.min_item_gas = gas;
        self
    }

    /// Minimum gas per incoming transfer.
    pub fn with_min_memo_gas(mut self, gas: u64) -> Self {
        self.service.min_memo_gas = gas;
        self
    }

    /// Footprint below which no deposit is owed.
    pub fn with_deposit_offset(mut self, offset: u64) -> Self {
        self.service.deposit_offset = JamAmount(offset);
        self
    }

    /// Host this blob (a path or a url) as a preimage, available from slot 0.
    pub fn with_preimage(mut self, location: impl Into<AssetLocation>) -> Self {
        self.service.preimages.push(location.into());
        self
    }

    /// Set a storage entry.
    pub fn with_storage(mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        self.service
            .storage
            .insert(JamBytes(key.into()), JamBytes(value.into()));
        self
    }

    fn build(self) -> Result<JamGenesisService, Vec<anyhow::Error>> {
        match self.code {
            Some(code) => Ok(JamGenesisService {
                code,
                ..self.service
            }),
            None => Err(vec![anyhow!("code: a service needs its code")]),
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

fn is_empty_bytes(value: &JamBytes) -> bool {
    value.0.is_empty()
}

/// (De)serialize a map with numeric keys through string keys: TOML table keys are strings.
mod string_keyed {
    use std::{collections::BTreeMap, fmt::Display, str::FromStr};

    use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<K, V, S>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        K: Display,
        V: Serialize,
        S: Serializer,
    {
        map.iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        K: FromStr + Ord,
        K::Err: Display,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        BTreeMap::<String, V>::deserialize(deserializer)?
            .into_iter()
            .map(|(k, v)| {
                k.parse()
                    .map(|k| (k, v))
                    .map_err(|e| de::Error::custom(format!("key '{k}': {e}")))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"
bootstrap_service = "/blobs/bootstrap.jam"

[services.5]
code = "/blobs/my-service.jam"
balance = "min+1000000"
min_item_gas = 10
deposit_offset = "max"
preimages = ["https://example.com/auth.jam"]
storage = { "0x6772656574696e67" = "0x68656c6c6f" }

[services.6]
code = "/blobs/other.jam"
balance = "18446744073709551615"

[auth_queues]
0 = [{ code = "/blobs/auth.jam", config = "0x0102" }]
1 = ["null", "0x0101010101010101010101010101010101010101010101010101010101010101"]

[privileges]
bless = 5
assign = { 0 = 5 }
always_acc = { 5 = 1000 }
"#;

    #[test]
    fn a_genesis_is_read_from_toml() {
        let genesis: JamGenesisConfig = toml::from_str(TOML).unwrap();

        assert!(genesis.with_defaults());
        assert_eq!(
            genesis.bootstrap_service(),
            Some(&AssetLocation::from("/blobs/bootstrap.jam"))
        );
        let service = &genesis.services()[&5];
        assert_eq!(service.balance(), JamBalance::MinimumPlus(1_000_000));
        assert_eq!(service.min_item_gas(), 10);
        assert_eq!(service.deposit_offset(), u64::MAX);
        assert_eq!(
            service.storage()[&JamBytes(b"greeting".to_vec())],
            JamBytes(b"hello".to_vec())
        );
        assert_eq!(
            genesis.services()[&6].balance(),
            JamBalance::Exact(u64::MAX)
        );
        assert_eq!(
            genesis.auth_queues()[&0],
            vec![JamAuthorizer::code_with_config("/blobs/auth.jam", [1, 2])]
        );
        assert_eq!(
            genesis.auth_queues()[&1],
            vec![JamAuthorizer::Null, JamAuthorizer::Hash([1; 32])]
        );
        assert_eq!(genesis.privileges().manager(), 5);
        assert_eq!(genesis.privileges().assign()[&0], 5);
        assert_eq!(genesis.privileges().always_acc()[&5], 1000);
    }

    #[test]
    fn a_genesis_round_trips_through_toml_and_json() {
        let genesis: JamGenesisConfig = toml::from_str(TOML).unwrap();

        let again: JamGenesisConfig = toml::from_str(&toml::to_string(&genesis).unwrap()).unwrap();
        assert_eq!(again, genesis);

        let again: JamGenesisConfig =
            serde_json::from_str(&serde_json::to_string(&genesis).unwrap()).unwrap();
        assert_eq!(again, genesis);
    }

    #[test]
    fn an_empty_genesis_is_the_default() {
        let genesis: JamGenesisConfig = toml::from_str("").unwrap();
        assert_eq!(genesis, JamGenesisConfig::default());
        assert!(genesis.with_defaults());
        assert_eq!(toml::to_string(&genesis).unwrap(), "");
    }

    #[test]
    fn balances_are_parsed() {
        for (text, expected) in [
            ("1000", JamBalance::Exact(1000)),
            ("1_000", JamBalance::Exact(1000)),
            ("max", JamBalance::Exact(u64::MAX)),
            ("min", JamBalance::MinimumPlus(0)),
            ("min+7", JamBalance::MinimumPlus(7)),
            ("min+max", JamBalance::MinimumPlus(u64::MAX)),
        ] {
            assert_eq!(text.parse::<JamBalance>().unwrap(), expected, "{text}");
            assert_eq!(
                expected.to_string().parse::<JamBalance>().unwrap(),
                expected
            );
        }
        assert!("min-1".parse::<JamBalance>().is_err());
        assert!("lots".parse::<JamBalance>().is_err());
    }

    #[test]
    fn bad_values_are_refused() {
        for bad in [
            "[auth_queues]\n0 = [\"0x01\"]",
            "[auth_queues]\nzero = [\"null\"]",
            "[services.5]\nbalance = 1",
            "[services.5]\ncode = \"a.jam\"\nstorage = { \"zz\" = \"0x00\" }",
            "[services.5]\ncode = \"a.jam\"\nbalance = -1",
        ] {
            assert!(
                toml::from_str::<JamGenesisConfig>(bad).is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn the_builder_matches_the_toml() {
        let built = JamGenesisConfigBuilder::new()
            .with_bootstrap_service("/blobs/bootstrap.jam")
            .with_service(5, |s| {
                s.with_code("/blobs/my-service.jam")
                    .with_balance(JamBalance::MinimumPlus(1_000_000))
                    .with_min_item_gas(10)
                    .with_deposit_offset(u64::MAX)
                    .with_preimage("https://example.com/auth.jam")
                    .with_storage(b"greeting".to_vec(), b"hello".to_vec())
            })
            .with_service(6, |s| {
                s.with_code("/blobs/other.jam").with_balance(u64::MAX)
            })
            .with_auth_queue(
                0,
                [JamAuthorizer::code_with_config("/blobs/auth.jam", [1, 2])],
            )
            .with_auth_queue(1, [JamAuthorizer::Null, JamAuthorizer::Hash([1; 32])])
            .with_manager(5)
            .with_assigner([0], 5)
            .with_always_acc(5, 1000)
            .build()
            .unwrap();

        assert_eq!(built, toml::from_str(TOML).unwrap());
    }

    #[test]
    fn the_builder_collects_errors() {
        let errors = JamGenesisConfigBuilder::new()
            .with_service(5, |s| s.with_min_item_gas(1))
            .with_protocol_parameters(serde_json::json!("tiny"))
            .build()
            .unwrap_err();

        assert_eq!(errors.len(), 2);
        assert!(errors[0].to_string().contains("services[5].code"));
        assert!(errors[1].to_string().contains("protocol_parameters"));
    }

    #[test]
    fn relative_paths_are_resolved_against_the_base_dir() {
        let mut genesis = JamGenesisConfigBuilder::new()
            .with_bootstrap_service("./bootstrap.jam")
            .with_service(5, |s| {
                s.with_code("svc.jam")
                    .with_preimage("/abs/pre.jam")
                    .with_preimage("https://example.com/x.jam")
            })
            .with_auth_queue(0, [JamAuthorizer::code("auth.jam")])
            .build()
            .unwrap();

        genesis.resolve_relative_paths(Path::new("/net"));

        assert_eq!(
            genesis.bootstrap_service(),
            Some(&AssetLocation::from("/net/./bootstrap.jam"))
        );
        let service = &genesis.services()[&5];
        assert_eq!(service.code(), &AssetLocation::from("/net/svc.jam"));
        assert_eq!(service.preimages()[0], AssetLocation::from("/abs/pre.jam"));
        assert_eq!(
            service.preimages()[1],
            AssetLocation::from("https://example.com/x.jam")
        );
        assert_eq!(
            genesis.auth_queues()[&0],
            vec![JamAuthorizer::code("/net/auth.jam")]
        );
    }
}
