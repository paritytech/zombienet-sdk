use std::{path::PathBuf, str::FromStr};

use lazy_static::lazy_static;
use regex::Regex;
use url::Url;

use super::errors::ConversionError;

pub type Duration = u32;

pub type Port = u16;

pub type ParaId = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct Chain(String);

impl TryFrom<&str> for Chain {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.contains(char::is_whitespace) {
            return Err(ConversionError::ContainsWhitespaces(value.to_string()));
        }

        Ok(Self(value.to_string()))
    }
}

impl Chain {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
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
            .expect("should compile with success. this is a bug, please report it");
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

#[derive(Debug, Clone, PartialEq)]
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

impl Command {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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

        Self::FilePath(
            PathBuf::from_str(value).expect("infaillible. this is a bug, please report it"),
        )
    }
}

/// A CLI argument, can be an option with an assigned value or a simple
/// flag to enable/disable a feature.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Flag(String),
    Option(String, String),
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

#[cfg(test)]
mod tests {
    use super::*;

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
