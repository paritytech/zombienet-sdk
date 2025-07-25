use std::{
    collections::HashMap,
    error::Error,
    fmt::{self, Display},
    path::PathBuf,
    str::FromStr,
};

use anyhow::anyhow;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{
    de::{self, IntoDeserializer},
    Deserialize, Deserializer, Serialize,
};
use support::constants::{INFAILABLE, SHOULD_COMPILE, THIS_IS_A_BUG};
use url::Url;

use super::{errors::ConversionError, resources::Resources};

/// An alias for a duration in seconds.
pub type Duration = u32;

/// An alias for a port.
pub type Port = u16;

/// An alias for a parachain ID.
pub type ParaId = u32;

/// Custom type wrapping u128 to add custom Serialization/Deserialization logic because it's not supported
/// issue tracking the problem: <https://github.com/toml-rs/toml/issues/540>
#[derive(Default, Debug, Clone, PartialEq)]
pub struct U128(pub(crate) u128);

impl From<u128> for U128 {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl TryFrom<&str> for U128 {
    type Error = Box<dyn Error>;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Self(value.to_string().parse::<u128>()?))
    }
}

impl Serialize for U128 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // here we add a prefix to the string to be able to replace the wrapped
        // value with "" to a value without "" in the TOML string
        serializer.serialize_str(&format!("U128%{}", self.0))
    }
}

struct U128Visitor;

impl de::Visitor<'_> for U128Visitor {
    type Value = U128;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an integer between 0 and 2^128 − 1.")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        v.try_into().map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for U128 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(U128Visitor)
    }
}

/// A chain name.
/// It can be constructed for an `&str`, if it fails, it will returns a [`ConversionError`].
///
/// # Examples:
/// ```
/// use zombienet_configuration::shared::types::Chain;
///
/// let polkadot: Chain = "polkadot".try_into().unwrap();
/// let kusama: Chain = "kusama".try_into().unwrap();
/// let myparachain: Chain = "myparachain".try_into().unwrap();
///
/// assert_eq!(polkadot.as_str(), "polkadot");
/// assert_eq!(kusama.as_str(), "kusama");
/// assert_eq!(myparachain.as_str(), "myparachain");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain(String);

impl TryFrom<&str> for Chain {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.contains(char::is_whitespace) {
            return Err(ConversionError::ContainsWhitespaces(value.to_string()));
        }

        if value.is_empty() {
            return Err(ConversionError::CantBeEmpty);
        }

        Ok(Self(value.to_string()))
    }
}

impl Chain {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A container image.
/// It can be constructed from an `&str` including a combination of name, version, IPv4 or/and hostname, if it fails, it will returns a [`ConversionError`].
///
/// # Examples:
/// ```
/// use zombienet_configuration::shared::types::Image;
///
/// let image1: Image = "name".try_into().unwrap();
/// let image2: Image = "name:version".try_into().unwrap();
/// let image3: Image = "myrepo.com/name:version".try_into().unwrap();
/// let image4: Image = "10.15.43.155/name:version".try_into().unwrap();
///
/// assert_eq!(image1.as_str(), "name");
/// assert_eq!(image2.as_str(), "name:version");
/// assert_eq!(image3.as_str(), "myrepo.com/name:version");
/// assert_eq!(image4.as_str(), "10.15.43.155/name:version");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image(String);

impl TryFrom<&str> for Image {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        static IP_PART: &str = "((([0-9]|[1-9][0-9]|1[0-9]{2}|2[0-4][0-9]|25[0-5]).){3}([0-9]|[1-9][0-9]|1[0-9]{2}|2[0-4][0-9]|25[0-5]))";
        static HOSTNAME_PART: &str = "((([a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9]).)*([A-Za-z0-9]|[A-Za-z0-9][A-Za-z0-9-]*[A-Za-z0-9]))";
        static TAG_NAME_PART: &str = "([a-z0-9](-*[a-z0-9])*)";
        static TAG_VERSION_PART: &str = "([a-z0-9_]([-._a-z0-9])*)";
        lazy_static! {
            static ref RE: Regex = Regex::new(&format!(
                "^({IP_PART}|{HOSTNAME_PART}/)?{TAG_NAME_PART}(:{TAG_VERSION_PART})?$",
            ))
            .expect(&format!("{SHOULD_COMPILE}, {THIS_IS_A_BUG}"));
        };

        if !RE.is_match(value) {
            return Err(ConversionError::DoesntMatchRegex {
                value: value.to_string(),
                regex: "^([ip]|[hostname]/)?[tag_name]:[tag_version]?$".to_string(),
            });
        }

        Ok(Self(value.to_string()))
    }
}

impl Image {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A command that will be executed natively (native provider) or in a container (podman/k8s).
/// It can be constructed from an `&str`, if it fails, it will returns a [`ConversionError`].
///
/// # Examples:
/// ```
/// use zombienet_configuration::shared::types::Command;
///
/// let command1: Command = "mycommand".try_into().unwrap();
/// let command2: Command = "myothercommand".try_into().unwrap();
///
/// assert_eq!(command1.as_str(), "mycommand");
/// assert_eq!(command2.as_str(), "myothercommand");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command(String);

impl TryFrom<&str> for Command {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.contains(char::is_whitespace) {
            return Err(ConversionError::ContainsWhitespaces(value.to_string()));
        }

        Ok(Self(value.to_string()))
    }
}
impl Default for Command {
    fn default() -> Self {
        Self(String::from("polkadot"))
    }
}

impl Command {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A command with optional custom arguments, the command will be executed natively (native provider) or in a container (podman/k8s).
/// It can be constructed from an `&str`, if it fails, it will returns a [`ConversionError`].
///
/// # Examples:
/// ```
/// use zombienet_configuration::shared::types::CommandWithCustomArgs;
///
/// let command1: CommandWithCustomArgs = "mycommand --demo=2 --other-flag".try_into().unwrap();
/// let command2: CommandWithCustomArgs = "my_other_cmd_without_args".try_into().unwrap();
///
/// assert_eq!(command1.cmd().as_str(), "mycommand");
/// assert_eq!(command2.cmd().as_str(), "my_other_cmd_without_args");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandWithCustomArgs(Command, Vec<Arg>);

impl TryFrom<&str> for CommandWithCustomArgs {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(ConversionError::CantBeEmpty);
        }

        let mut parts = value.split_whitespace().collect::<Vec<&str>>();
        let cmd = parts.remove(0).try_into().unwrap();
        let args = parts
            .iter()
            .map(|x| {
                Arg::deserialize(x.into_deserializer()).map_err(|_: serde_json::Error| {
                    ConversionError::DeserializeError(String::from(*x))
                })
            })
            .collect::<Result<Vec<Arg>, _>>()?;

        Ok(Self(cmd, args))
    }
}
impl Default for CommandWithCustomArgs {
    fn default() -> Self {
        Self("polkadot".try_into().unwrap(), vec![])
    }
}

impl CommandWithCustomArgs {
    pub fn cmd(&self) -> &Command {
        &self.0
    }

    pub fn args(&self) -> &Vec<Arg> {
        &self.1
    }
}

/// A location for a locally or remotely stored asset.
/// It can be constructed from an [`url::Url`], a [`std::path::PathBuf`] or an `&str`.
///
/// # Examples:
/// ```
/// use url::Url;
/// use std::{path::PathBuf, str::FromStr};
/// use zombienet_configuration::shared::types::AssetLocation;
///
/// let url_location: AssetLocation = Url::from_str("https://mycloudstorage.com/path/to/my/file.tgz").unwrap().into();
/// let url_location2: AssetLocation = "https://mycloudstorage.com/path/to/my/file.tgz".into();
/// let path_location: AssetLocation = PathBuf::from_str("/tmp/path/to/my/file").unwrap().into();
/// let path_location2: AssetLocation = "/tmp/path/to/my/file".into();
///
/// assert!(matches!(url_location, AssetLocation::Url(value) if value.as_str() == "https://mycloudstorage.com/path/to/my/file.tgz"));
/// assert!(matches!(url_location2, AssetLocation::Url(value) if value.as_str() == "https://mycloudstorage.com/path/to/my/file.tgz"));
/// assert!(matches!(path_location, AssetLocation::FilePath(value) if value.to_str().unwrap() == "/tmp/path/to/my/file"));
/// assert!(matches!(path_location2, AssetLocation::FilePath(value) if value.to_str().unwrap() == "/tmp/path/to/my/file"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum AssetLocation {
    Url(Url),
    FilePath(PathBuf),
}

impl From<Url> for AssetLocation {
    fn from(value: Url) -> Self {
        Self::Url(value)
    }
}

impl From<PathBuf> for AssetLocation {
    fn from(value: PathBuf) -> Self {
        Self::FilePath(value)
    }
}

impl From<&str> for AssetLocation {
    fn from(value: &str) -> Self {
        if let Ok(parsed_url) = Url::parse(value) {
            return Self::Url(parsed_url);
        }

        Self::FilePath(PathBuf::from_str(value).expect(&format!("{INFAILABLE}, {THIS_IS_A_BUG}")))
    }
}

impl Display for AssetLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetLocation::Url(value) => write!(f, "{}", value.as_str()),
            AssetLocation::FilePath(value) => write!(f, "{}", value.display()),
        }
    }
}

impl AssetLocation {
    pub async fn get_asset(&self) -> Result<Vec<u8>, anyhow::Error> {
        let contents = match self {
            AssetLocation::Url(location) => {
                let res = reqwest::get(location.as_ref()).await.map_err(|err| {
                    anyhow!(
                        "Error dowinloding asset from url {} - {}",
                        location,
                        err.to_string()
                    )
                })?;

                res.bytes().await.unwrap().into()
            },
            AssetLocation::FilePath(filepath) => {
                tokio::fs::read(filepath).await.map_err(|err| {
                    anyhow!(
                        "Error reading asset from path {} - {}",
                        filepath.to_string_lossy(),
                        err.to_string()
                    )
                })?
            },
        };

        Ok(contents)
    }
}

impl Serialize for AssetLocation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct AssetLocationVisitor;

impl de::Visitor<'_> for AssetLocationVisitor {
    type Value = AssetLocation;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(AssetLocation::from(v))
    }
}

impl<'de> Deserialize<'de> for AssetLocation {
    fn deserialize<D>(deserializer: D) -> Result<AssetLocation, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(AssetLocationVisitor)
    }
}

/// A CLI argument passed to an executed command, can be an option with an assigned value or a simple flag to enable/disable a feature.
/// A flag arg can be constructed from a `&str` and a option arg can be constructed from a `(&str, &str)`.
///
/// # Examples:
/// ```
/// use zombienet_configuration::shared::types::Arg;
///
/// let flag_arg: Arg = "myflag".into();
/// let option_arg: Arg = ("name", "value").into();
///
/// assert!(matches!(flag_arg, Arg::Flag(value) if value == "myflag"));
/// assert!(matches!(option_arg, Arg::Option(name, value) if name == "name" && value == "value"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Flag(String),
    Option(String, String),
    Array(String, Vec<String>),
}

impl From<&str> for Arg {
    fn from(flag: &str) -> Self {
        Self::Flag(flag.to_owned())
    }
}

impl From<(&str, &str)> for Arg {
    fn from((option, value): (&str, &str)) -> Self {
        Self::Option(option.to_owned(), value.to_owned())
    }
}

impl<T> From<(&str, &[T])> for Arg
where
    T: AsRef<str> + Clone,
{
    fn from((option, values): (&str, &[T])) -> Self {
        Self::Array(
            option.to_owned(),
            values.iter().map(|v| v.as_ref().to_string()).collect(),
        )
    }
}

impl<T> From<(&str, Vec<T>)> for Arg
where
    T: AsRef<str>,
{
    fn from((option, values): (&str, Vec<T>)) -> Self {
        Self::Array(
            option.to_owned(),
            values.into_iter().map(|v| v.as_ref().to_string()).collect(),
        )
    }
}

impl Serialize for Arg {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Arg::Flag(value) => serializer.serialize_str(value),
            Arg::Option(option, value) => serializer.serialize_str(&format!("{option}={value}")),
            Arg::Array(option, values) => {
                serializer.serialize_str(&format!("{}=[{}]", option, values.join(",")))
            },
        }
    }
}

struct ArgVisitor;

impl de::Visitor<'_> for ArgVisitor {
    type Value = Arg;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        // covers the "-lruntime=debug,parachain=trace" case
        // TODO: Make this more generic by adding the scenario in the regex below
        if v.starts_with("-l") || v.starts_with("-log") {
            return Ok(Arg::Flag(v.to_string()));
        }
        let re = Regex::new("^(?<name_prefix>(?<prefix>-{1,2})?(?<name>[a-zA-Z]+(-[a-zA-Z]+)*))((?<separator>=| )(?<value>\\[[^\\]]*\\]|[^ ]+))?$").unwrap();

        let captures = re.captures(v);
        if let Some(captures) = captures {
            if let Some(value) = captures.name("value") {
                let name_prefix = captures
                    .name("name_prefix")
                    .expect("BUG: name_prefix capture group missing")
                    .as_str()
                    .to_string();

                let val = value.as_str();
                if val.starts_with('[') && val.ends_with(']') {
                    // Remove brackets and split by comma
                    let inner = &val[1..val.len() - 1];
                    let items: Vec<String> = inner
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    return Ok(Arg::Array(name_prefix, items));
                } else {
                    return Ok(Arg::Option(name_prefix, val.to_string()));
                }
            }
            if let Some(name_prefix) = captures.name("name_prefix") {
                return Ok(Arg::Flag(name_prefix.as_str().to_string()));
            }
        }

        Err(de::Error::custom(
            "the provided argument is invalid and doesn't match Arg::Option, Arg::Flag or Arg::Array",
        ))
    }
}

impl<'de> Deserialize<'de> for Arg {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ArgVisitor)
    }
}

#[derive(Debug, Default, Clone)]
pub struct ValidationContext {
    pub used_ports: Vec<Port>,
    pub used_nodes_names: Vec<String>,
    // Store para_id already used
    pub used_para_ids: HashMap<ParaId, u8>,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct ChainDefaultContext {
    pub(crate) default_command: Option<Command>,
    pub(crate) default_image: Option<Image>,
    pub(crate) default_resources: Option<Resources>,
    pub(crate) default_db_snapshot: Option<AssetLocation>,
    #[serde(default)]
    pub(crate) default_args: Vec<Arg>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_flag_roundtrip() {
        let arg = Arg::from("verbose");
        let serialized = serde_json::to_string(&arg).unwrap();
        let deserialized: Arg = serde_json::from_str(&serialized).unwrap();
        assert_eq!(arg, deserialized);
    }
    #[test]
    fn test_arg_option_roundtrip() {
        let arg = Arg::from(("mode", "fast"));
        let serialized = serde_json::to_string(&arg).unwrap();
        let deserialized: Arg = serde_json::from_str(&serialized).unwrap();
        assert_eq!(arg, deserialized);
    }

    #[test]
    fn test_arg_array_roundtrip() {
        let arg = Arg::from(("items", ["a", "b", "c"].as_slice()));

        let serialized = serde_json::to_string(&arg).unwrap();
        println!("serialized = {serialized}");
        let deserialized: Arg = serde_json::from_str(&serialized).unwrap();
        assert_eq!(arg, deserialized);
    }

    #[test]
    fn test_arg_option_valid_input() {
        let expected = Arg::from(("--foo", "bar"));

        // name and value delimited with =
        let valid = "\"--foo=bar\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);

        // name and value delimited with space
        let valid = "\"--foo bar\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);

        // value contains =
        let expected = Arg::from(("--foo", "bar=baz"));
        let valid = "\"--foo=bar=baz\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_arg_array_valid_input() {
        let expected = Arg::from(("--foo", vec!["bar", "baz"]));

        // name and values delimited with =
        let valid = "\"--foo=[bar,baz]\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);

        // name and values delimited with space
        let valid = "\"--foo [bar,baz]\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);

        // values delimited with commas and space
        let valid = "\"--foo [bar , baz]\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);

        // empty values array
        let expected = Arg::from(("--foo", Vec::<&str>::new()));
        let valid = "\"--foo []\"";
        let result: Result<Arg, _> = serde_json::from_str(valid);
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_arg_invalid_input() {
        // missing = or space
        let invalid = "\"--foo[bar]\"";
        let result: Result<Arg, _> = serde_json::from_str(invalid);
        assert!(result.is_err());

        // value contains space
        let invalid = "\"--foo=bar baz\"";
        let result: Result<Arg, _> = serde_json::from_str(invalid);
        println!("result = {result:?}");
        assert!(result.is_err());
    }

    #[test]
    fn converting_a_str_without_whitespaces_into_a_chain_should_succeeds() {
        let got: Result<Chain, ConversionError> = "mychain".try_into();

        assert_eq!(got.unwrap().as_str(), "mychain");
    }

    #[test]
    fn converting_a_str_containing_tag_name_into_an_image_should_succeeds() {
        let got: Result<Image, ConversionError> = "myimage".try_into();

        assert_eq!(got.unwrap().as_str(), "myimage");
    }

    #[test]
    fn converting_a_str_containing_tag_name_and_tag_version_into_an_image_should_succeeds() {
        let got: Result<Image, ConversionError> = "myimage:version".try_into();

        assert_eq!(got.unwrap().as_str(), "myimage:version");
    }

    #[test]
    fn converting_a_str_containing_hostname_and_tag_name_into_an_image_should_succeeds() {
        let got: Result<Image, ConversionError> = "myrepository.com/myimage".try_into();

        assert_eq!(got.unwrap().as_str(), "myrepository.com/myimage");
    }

    #[test]
    fn converting_a_str_containing_hostname_tag_name_and_tag_version_into_an_image_should_succeeds()
    {
        let got: Result<Image, ConversionError> = "myrepository.com/myimage:version".try_into();

        assert_eq!(got.unwrap().as_str(), "myrepository.com/myimage:version");
    }

    #[test]
    fn converting_a_str_containing_ip_and_tag_name_into_an_image_should_succeeds() {
        let got: Result<Image, ConversionError> = "myrepository.com/myimage".try_into();

        assert_eq!(got.unwrap().as_str(), "myrepository.com/myimage");
    }

    #[test]
    fn converting_a_str_containing_ip_tag_name_and_tag_version_into_an_image_should_succeeds() {
        let got: Result<Image, ConversionError> = "127.0.0.1/myimage:version".try_into();

        assert_eq!(got.unwrap().as_str(), "127.0.0.1/myimage:version");
    }

    #[test]
    fn converting_a_str_without_whitespaces_into_a_command_should_succeeds() {
        let got: Result<Command, ConversionError> = "mycommand".try_into();

        assert_eq!(got.unwrap().as_str(), "mycommand");
    }

    #[test]
    fn converting_an_url_into_an_asset_location_should_succeeds() {
        let url = Url::from_str("https://mycloudstorage.com/path/to/my/file.tgz").unwrap();
        let got: AssetLocation = url.clone().into();

        assert!(matches!(got, AssetLocation::Url(value) if value == url));
    }

    #[test]
    fn converting_a_pathbuf_into_an_asset_location_should_succeeds() {
        let pathbuf = PathBuf::from_str("/tmp/path/to/my/file").unwrap();
        let got: AssetLocation = pathbuf.clone().into();

        assert!(matches!(got, AssetLocation::FilePath(value) if value == pathbuf));
    }

    #[test]
    fn converting_a_str_into_an_url_asset_location_should_succeeds() {
        let url = "https://mycloudstorage.com/path/to/my/file.tgz";
        let got: AssetLocation = url.into();

        assert!(matches!(got, AssetLocation::Url(value) if value == Url::from_str(url).unwrap()));
    }

    #[test]
    fn converting_a_str_into_an_filepath_asset_location_should_succeeds() {
        let filepath = "/tmp/path/to/my/file";
        let got: AssetLocation = filepath.into();

        assert!(matches!(
            got,
            AssetLocation::FilePath(value) if value == PathBuf::from_str(filepath).unwrap()
        ));
    }

    #[test]
    fn converting_a_str_into_an_flag_arg_should_succeeds() {
        let got: Arg = "myflag".into();

        assert!(matches!(got, Arg::Flag(flag) if flag == "myflag"));
    }

    #[test]
    fn converting_a_str_tuple_into_an_option_arg_should_succeeds() {
        let got: Arg = ("name", "value").into();

        assert!(matches!(got, Arg::Option(name, value) if name == "name" && value == "value"));
    }

    #[test]
    fn converting_a_str_with_whitespaces_into_a_chain_should_fails() {
        let got: Result<Chain, ConversionError> = "my chain".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::ContainsWhitespaces(_)
        ));
        assert_eq!(
            got.unwrap_err().to_string(),
            "'my chain' shouldn't contains whitespace"
        );
    }

    #[test]
    fn converting_an_empty_str_into_a_chain_should_fails() {
        let got: Result<Chain, ConversionError> = "".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::CantBeEmpty
        ));
        assert_eq!(got.unwrap_err().to_string(), "can't be empty");
    }

    #[test]
    fn converting_a_str_containing_only_ip_into_an_image_should_fails() {
        let got: Result<Image, ConversionError> = "127.0.0.1".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::DoesntMatchRegex { value: _, regex: _ }
        ));
        assert_eq!(
            got.unwrap_err().to_string(),
            "'127.0.0.1' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'"
        );
    }

    #[test]
    fn converting_a_str_containing_only_ip_and_tag_version_into_an_image_should_fails() {
        let got: Result<Image, ConversionError> = "127.0.0.1:version".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::DoesntMatchRegex { value: _, regex: _ }
        ));
        assert_eq!(got.unwrap_err().to_string(), "'127.0.0.1:version' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'");
    }

    #[test]
    fn converting_a_str_containing_only_hostname_into_an_image_should_fails() {
        let got: Result<Image, ConversionError> = "myrepository.com".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::DoesntMatchRegex { value: _, regex: _ }
        ));
        assert_eq!(got.unwrap_err().to_string(), "'myrepository.com' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'");
    }

    #[test]
    fn converting_a_str_containing_only_hostname_and_tag_version_into_an_image_should_fails() {
        let got: Result<Image, ConversionError> = "myrepository.com:version".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::DoesntMatchRegex { value: _, regex: _ }
        ));
        assert_eq!(got.unwrap_err().to_string(), "'myrepository.com:version' doesn't match regex '^([ip]|[hostname]/)?[tag_name]:[tag_version]?$'");
    }

    #[test]
    fn converting_a_str_with_whitespaces_into_a_command_should_fails() {
        let got: Result<Command, ConversionError> = "my command".try_into();

        assert!(matches!(
            got.clone().unwrap_err(),
            ConversionError::ContainsWhitespaces(_)
        ));
        assert_eq!(
            got.unwrap_err().to_string(),
            "'my command' shouldn't contains whitespace"
        );
    }
}
