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

#[derive(Debug)]
pub struct ResourcesBuilder {
    config: Resources,
}

impl ResourcesBuilder {
    pub fn new() -> ResourcesBuilder {
        Self {
            config: Resources {
                request_memory: None,
                request_cpu: None,
                limit_memory: None,
                limit_cpu: None,
            },
        }
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
