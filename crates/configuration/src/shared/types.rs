#[derive(Debug, Clone)]
pub struct MultiAddress(String);

#[derive(Debug, Clone)]
pub struct IpAddress(String);

#[derive(Debug, Clone)]
pub struct Duration(String);

#[derive(Debug, Clone)]
pub struct Command(String);

#[derive(Debug, Clone)]
pub struct ContainerImage(String);

#[derive(Debug, Clone)]
pub struct Port(u16);

#[derive(Debug, Clone)]
pub struct ParaId(u32);

#[derive(Debug, Clone)]
pub enum ResourceQuantity {
    Memory(String),
    Cpu(String),
}

#[derive(Debug, Default, Clone)]
pub struct Resources {
    request_memory: Option<ResourceQuantity>,
    request_cpu: Option<ResourceQuantity>,
    limit_memory: Option<ResourceQuantity>,
    limit_cpu: Option<ResourceQuantity>,
}

impl Resources {
    pub fn with_request_memory(self, quantity: ResourceQuantity) -> Self {
        Self {
            request_memory: Some(quantity),
            ..self
        }
    }

    pub fn with_request_cpu(self, quantity: ResourceQuantity) -> Self {
        Self {
            request_cpu: Some(quantity),
            ..self
        }
    }

    pub fn with_limit_memory(self, quantity: ResourceQuantity) -> Self {
        Self {
            limit_memory: Some(quantity),
            ..self
        }
    }

    pub fn with_limit_cpu(self, quantity: ResourceQuantity) -> Self {
        Self {
            limit_cpu: Some(quantity),
            ..self
        }
    }
}

#[derive(Debug, Clone)]
pub enum AssetLocation {
    URL(String),
    FilePath(String),
}

/// A CLI argument, can be an option with an assigned value or a simple
/// flag to enable/disable a feature.
#[derive(Debug, Clone)]
pub enum Arg {
    Flag(String),
    Option(String, String),
}

impl From<String> for Arg {
    fn from(flag: String) -> Self {
        Self::Flag(flag)
    }
}

impl From<(String, String)> for Arg {
    fn from((option, value): (String, String)) -> Self {
        Self::Option(option, value)
    }
}
