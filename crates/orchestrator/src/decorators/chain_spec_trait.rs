/// Trait that all decorators must implement
///
/// Decorators customize chain specs by implementing `customize_relay` and/or `customize_para`.
///
/// Specialized methods (clear_authorities, add_authorities, etc.) are optional:
/// - Return `None` (default) = use the default chain-spec behavior
/// - Return `Some(Ok(()))` = use custom implementation
/// - Return `Some(Err(...))` = custom implementation failed
pub trait ChainSpecDecorator: Send + Sync {
    /// Unique name for this decorator
    fn name(&self) -> &str;

    /// Customize relay chain spec
    fn customize_relay(&self, _spec: &mut serde_json::Value) -> Result<(), anyhow::Error> {
        Ok(()) // Default no-op
    }

    /// Customize parachain spec
    fn customize_para(&self, _spec: &mut serde_json::Value) -> Result<(), anyhow::Error> {
        Ok(()) // Default no-op
    }

    /// Optional: Custom authority clearing logic
    fn clear_authorities(
        &self,
        _spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        None // Default = use default chain-spec clearing
    }

    /// Optional: Custom authority adding logic
    fn add_authorities(&self, _spec: &mut serde_json::Value) -> Option<Result<(), anyhow::Error>> {
        None // Default = use default chain-spec behavior
    }

    /// Optional: Custom aura authorities adding logic
    fn add_aura_authorities(
        &self,
        _spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        None
    }

    /// Optional: Custom grandpa authorities adding logic
    fn add_grandpa_authorities(
        &self,
        _spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        None
    }

    /// Optional: Custom collator selection adding logic
    fn add_collator_selection(
        &self,
        _spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        None
    }

    /// Optional: Custom balances adding logic
    fn add_balances(&self, _spec: &mut serde_json::Value) -> Option<Result<(), anyhow::Error>> {
        None
    }

    /// Optional: Custom staking adding logic
    fn add_staking(&self, _spec: &mut serde_json::Value) -> Option<Result<(), anyhow::Error>> {
        None
    }

    /// Optional: Custom HRMP channels adding logic
    fn add_hrmp_channels(
        &self,
        _spec: &mut serde_json::Value,
    ) -> Option<Result<(), anyhow::Error>> {
        None
    }
}
