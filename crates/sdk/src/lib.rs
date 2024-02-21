use async_trait::async_trait;
pub use configuration::{NetworkConfig, NetworkConfigBuilder, RegistrationStrategy};
pub use orchestrator::{
    errors::OrchestratorError,
    network::{node::NetworkNode, Network},
    AddCollatorOptions, AddNodeOptions, Orchestrator, PjsResult,
};
pub use support::fs::local::LocalFileSystem;

use provider::{KubernetesProvider, NativeProvider};

#[async_trait]
pub trait NetworkConfigExt {
    /// Spawns a network using the native or k8s provider.
    ///
    /// # Example:
    /// ```rust
    /// # use zombienet_sdk::{NetworkConfig, NetworkConfigExt};
    /// # async fn example() -> Result<(), zombienet_sdk::OrchestratorError> {
    /// let network = NetworkConfig::load_from_toml("config.toml")?
    ///     .spawn_native()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn spawn_native(self) -> Result<Network<LocalFileSystem>, OrchestratorError>;
    async fn spawn_k8s(self) -> Result<Network<LocalFileSystem>, OrchestratorError>;
}

#[async_trait]
impl NetworkConfigExt for NetworkConfig {
    async fn spawn_native(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let provider = NativeProvider::new(LocalFileSystem {});
        let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
        orchestrator.spawn(self).await
    }

    async fn spawn_k8s(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let provider = KubernetesProvider::new(LocalFileSystem {}).await;
        let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
        orchestrator.spawn(self).await
    }
}
