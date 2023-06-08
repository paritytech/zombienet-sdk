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

#[derive(Debug, Clone, PartialEq)]
pub struct Resources {
    request_memory: Option<ResourceQuantity>,
    request_cpu:    Option<ResourceQuantity>,
    limit_memory:   Option<ResourceQuantity>,
    limit_cpu:      Option<ResourceQuantity>,
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

#[derive(Debug)]
pub struct ResourcesBuilder {
    config: Resources,
}

impl Default for ResourcesBuilder {
    fn default() -> Self {
        Self {
            config: Resources {
                request_memory: None,
                request_cpu:    None,
                limit_memory:   None,
                limit_cpu:      None,
            },
        }
    }
}

impl ResourcesBuilder {
    pub fn new() -> ResourcesBuilder {
        Self::default()
    }

    fn transition(config: Resources) -> Self {
        Self { config }
    }

    pub fn with_request_memory(self, quantity: &str) -> Self {
        Self::transition(Resources {
            request_memory: Some(ResourceQuantity(quantity.to_owned())),
            ..self.config
        })
    }

    pub fn with_request_cpu(self, quantity: &str) -> Self {
        Self::transition(Resources {
            request_cpu: Some(ResourceQuantity(quantity.to_owned())),
            ..self.config
        })
    }

    pub fn with_limit_memory(self, quantity: &str) -> Self {
        Self::transition(Resources {
            limit_memory: Some(ResourceQuantity(quantity.to_owned())),
            ..self.config
        })
    }

    pub fn with_limit_cpu(self, quantity: &str) -> Self {
        Self::transition(Resources {
            limit_cpu: Some(ResourceQuantity(quantity.to_owned())),
            ..self.config
        })
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

        assert_eq!(resources.request_memory().unwrap().value(), "200M");
        assert_eq!(resources.request_cpu().unwrap().value(), "1G");
        assert_eq!(resources.limit_cpu().unwrap().value(), "500M");
        assert_eq!(resources.limit_memory().unwrap().value(), "2G");
    }
}
