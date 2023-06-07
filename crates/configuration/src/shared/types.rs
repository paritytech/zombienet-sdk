use std::{path::PathBuf, str::FromStr};

use url::Url;

pub type Duration = u32;

pub type Port = u16;

pub type ParaId = u32;

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
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Ok(parsed_url) = Url::parse(value) {
            return Ok(Self::Url(parsed_url));
        }

        if let Ok(parsed_path) = PathBuf::from_str(value) {
            return Ok(Self::FilePath(parsed_path));
        }

        Err("unable to convert into url::Url or path::PathBuf".to_string())
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
