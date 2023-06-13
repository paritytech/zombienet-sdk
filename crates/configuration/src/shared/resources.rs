use lazy_static::lazy_static;
use regex::Regex;

use super::helpers::merge_errors;

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceQuantity(String);

impl ResourceQuantity {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ResourceQuantity {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"^\d+(.\d+)?(m|K|M|G|T|P|E|Ki|Mi|Gi|Ti|Pi|Ei)?$")
                .expect("compile with success");
        }

        if !RE.is_match(value) {
            return Err("".to_string());
        }

        Ok(Self(value.to_string()))
    }
}

impl From<u64> for ResourceQuantity {
    fn from(value: u64) -> Self {
        Self(value.to_string())
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

#[derive(Debug, Default)]
pub struct ResourcesBuilder {
    config: Resources,
    errors: Vec<String>,
}

impl ResourcesBuilder {
    pub fn new() -> ResourcesBuilder {
        Self::default()
    }

    fn transition(config: Resources, errors: Vec<String>) -> Self {
        Self { config, errors }
    }

    pub fn with_request_memory(self, quantity: impl TryInto<ResourceQuantity>) -> Self {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    request_memory: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(_error) => Self::transition(
                self.config,
                // merge_errors(self.errors, format!("request_memory: {error}")),
                merge_errors(self.errors, format!("request_memory: ")),
            ),
        }
    }

    pub fn with_request_cpu(self, quantity: impl TryInto<ResourceQuantity>) -> Self {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    request_cpu: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(_error) => Self::transition(
                self.config,
                // merge_errors(self.errors, format!("request_cpu: {error}")),
                merge_errors(self.errors, format!("request_cpu: ")),
            ),
        }
    }

    pub fn with_limit_memory(self, quantity: impl TryInto<ResourceQuantity>) -> Self {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    limit_memory: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(_error) => Self::transition(
                self.config,
                // merge_errors(self.errors, format!("limit_memory: {error}")),
                merge_errors(self.errors, format!("limit_memory: ")),
            ),
        }
    }

    pub fn with_limit_cpu(self, quantity: impl TryInto<ResourceQuantity>) -> Self {
        match quantity.try_into() {
            Ok(quantity) => Self::transition(
                Resources {
                    limit_cpu: Some(quantity),
                    ..self.config
                },
                self.errors,
            ),
            Err(_error) => Self::transition(
                self.config,
                // merge_errors(self.errors, format!("limit_cpu: {error}")),
                merge_errors(self.errors, format!("limit_cpu: ")),
            ),
        }
    }

    pub fn build(self) -> Resources {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_config_builder_should_build_a_new_resources_config_correctly() {
        let resources = ResourcesBuilder::new()
            .with_request_memory("200M")
            .with_request_cpu("1G")
            .with_limit_cpu("500M")
            .with_limit_memory("2G")
            .build();

        assert_eq!(resources.request_memory().unwrap().as_str(), "200M");
        assert_eq!(resources.request_cpu().unwrap().as_str(), "1G");
        assert_eq!(resources.limit_cpu().unwrap().as_str(), "500M");
        assert_eq!(resources.limit_memory().unwrap().as_str(), "2G");
    }
}
