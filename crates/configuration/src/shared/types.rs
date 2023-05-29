#[derive(Debug, Clone, PartialEq)]
pub struct MultiAddress(String);

impl MultiAddress {
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl From<&str> for MultiAddress {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IpAddress(String);

pub type Duration = u32;

pub type Port = u16;

pub type ParaId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum AssetLocation {
    Url(String),
    FilePath(String),
}

impl AssetLocation {
    pub fn value(&self) -> &str {
        match self {
            AssetLocation::Url(value) => value,
            AssetLocation::FilePath(value) => value,
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
