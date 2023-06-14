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
            .expect("compile with succes");
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

impl TryFrom<&str> for AssetLocation {
    type Error = ConversionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Ok(parsed_url) = Url::parse(value) {
            return Ok(Self::Url(parsed_url));
        }

        if let Ok(parsed_path) = PathBuf::from_str(value) {
            return Ok(Self::FilePath(parsed_path));
        }

        Err(ConversionError::InvalidUrlOrPathBuf(value.to_string()))
    }
}

impl AssetLocation {
    pub fn as_url(&self) -> Option<&Url> {
        if let Self::Url(url) = self {
            Some(url)
        } else {
            None
        }
    }

    pub fn as_path_buf(&self) -> Option<&PathBuf> {
        if let Self::FilePath(path) = self {
            Some(path)
        } else {
            None
        }
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
