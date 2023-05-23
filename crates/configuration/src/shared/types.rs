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

#[derive(Debug, Clone)]
pub struct IpAddress(String);

#[derive(Debug, Clone)]
pub struct Duration(String);

pub type Port = u16;

pub type ParaId = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceQuantity(String);

impl ResourceQuantity {
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ResourceQuantity {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Resources {
    request_memory: Option<ResourceQuantity>,
    request_cpu: Option<ResourceQuantity>,
    limit_memory: Option<ResourceQuantity>,
    limit_cpu: Option<ResourceQuantity>,
}

impl Resources {
    pub fn with_request_memory(self, quantity: &str) -> Self {
        Self {
            request_memory: Some(ResourceQuantity(quantity.to_owned())),
            ..self
        }
    }

    pub fn with_request_cpu(self, quantity: &str) -> Self {
        Self {
            request_cpu: Some(ResourceQuantity(quantity.to_owned())),
            ..self
        }
    }

    pub fn with_limit_memory(self, quantity: &str) -> Self {
        Self {
            limit_memory: Some(ResourceQuantity(quantity.to_owned())),
            ..self
        }
    }

    pub fn with_limit_cpu(self, quantity: &str) -> Self {
        Self {
            limit_cpu: Some(ResourceQuantity(quantity.to_owned())),
            ..self
        }
    }

    pub fn request_memory(&self) -> Option<&ResourceQuantity> {
        self.request_memory.as_ref()
    }

    pub fn request_cpu(&self) -> Option<&ResourceQuantity> {
        self.request_cpu.as_ref()
    }

    pub fn limit_memory(&self) -> Option<&ResourceQuantity> {
        self.limit_memory.as_ref()
    }

    pub fn limit_cpu(&self) -> Option<&ResourceQuantity> {
        self.limit_cpu.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssetLocation {
    Url(String),
    FilePath(String),
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
