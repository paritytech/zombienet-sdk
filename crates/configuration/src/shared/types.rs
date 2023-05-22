pub type MultiAddress = String;

pub type IpAddress = String;

pub type Timeout = u16;

pub type Command = String;

pub type ContainerImage = String;

pub type Port = u16;

#[derive(Debug, Clone)]
pub enum ResourceQuantity {
    Memory(String),
    Cpu(String),
}

#[derive(Debug, Clone)]
pub struct Resources {
    request_memory: Option<ResourceQuantity>,
    request_cpu:    Option<ResourceQuantity>,
    limit_memory:   Option<ResourceQuantity>,
    limit_cpu:      Option<ResourceQuantity>,
}

#[derive(Debug, Clone)]
pub enum DbSnapshot {
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
