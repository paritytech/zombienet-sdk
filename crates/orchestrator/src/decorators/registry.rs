use std::{collections::HashMap, sync::Arc};

use super::chain_spec_trait::ChainSpecDecorator;

/// Runtime chain type for applying decorators
#[derive(Debug, Clone, Copy)]
pub enum ChainType {
    Relay,
    Para,
}

/// Registry for managing decorators
#[derive(Clone)]
pub struct DecoratorRegistry {
    decorators: HashMap<String, Arc<dyn ChainSpecDecorator>>,
}

impl std::fmt::Debug for DecoratorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoratorRegistry")
            .field("decorators", &self.decorators.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl DecoratorRegistry {
    pub fn new() -> Self {
        Self {
            decorators: HashMap::new(),
        }
    }

    /// Register a trait-based decorator
    pub fn register(&mut self, decorator: Arc<dyn ChainSpecDecorator>) {
        let name = decorator.name().to_string();
        self.decorators.insert(name, decorator);
    }

    /// Apply a specific decorator by name
    pub fn apply(
        &self,
        name: &str,
        spec: &mut serde_json::Value,
        chain_type: ChainType,
    ) -> Result<(), anyhow::Error> {
        if let Some(decorator) = self.decorators.get(name) {
            match chain_type {
                ChainType::Relay => decorator.customize_relay(spec)?,
                ChainType::Para => decorator.customize_para(spec)?,
            }
        }

        Ok(())
    }

    /// Apply all decorators
    pub fn apply_all(
        &self,
        spec: &mut serde_json::Value,
        chain_type: ChainType,
    ) -> Result<(), anyhow::Error> {
        for decorator in self.decorators.values() {
            match chain_type {
                ChainType::Relay => decorator.customize_relay(spec)?,
                ChainType::Para => decorator.customize_para(spec)?,
            }
        }

        Ok(())
    }

    /// Apply clear_authorities from decorators (if any override it)
    /// Returns None if no decorator provides clear_authorities override
    pub fn apply_clear_authorities(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.clear_authorities(spec) {
                return Some(result);
            }
        }

        None
    }

    /// Apply add_authorities from decorators (if any override it)
    pub fn apply_add_authorities(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_authorities(spec) {
                return Some(result);
            }
        }

        None
    }

    /// Apply add_aura_authorities from decorators (if any override it)
    pub fn apply_add_aura_authorities(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_aura_authorities(spec) {
                return Some(result);
            }
        }

        None
    }

    /// Apply add_grandpa_authorities from decorators (if any override it)
    pub fn apply_add_grandpa_authorities(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_grandpa_authorities(spec) {
                return Some(result);
            }
        }

        None
    }

    /// Apply add_collator_selection from decorators (if any override it)
    pub fn apply_add_collator_selection(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_collator_selection(spec) {
                return Some(result);
            }
        }

        None
    }

    /// Apply add_balances from decorators (if any override it)
    pub fn apply_add_balances(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_balances(spec) {
                return Some(result);
            }
        }
        None
    }

    /// Apply add_staking from decorators (if any override it)
    pub fn apply_add_staking(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_staking(spec) {
                return Some(result);
            }
        }
        None
    }

    /// Apply add_hrmp_channels from decorators (if any override it)
    pub fn apply_add_hrmp_channels(
        &self,
        spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        for decorator in self.decorators.values() {
            if let Some(result) = decorator.add_hrmp_channels(spec) {
                return Some(result);
            }
        }

        None
    }

    /// Get decorator names
    pub fn names(&self) -> Vec<String> {
        self.decorators.keys().cloned().collect()
    }
}

impl Default for DecoratorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
