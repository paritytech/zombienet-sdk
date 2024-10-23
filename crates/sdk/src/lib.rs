use async_trait::async_trait;
pub use configuration::{NetworkConfig, NetworkConfigBuilder, RegistrationStrategy};
#[cfg(feature = "pjs")]
pub use orchestrator::pjs_helper::PjsResult;
pub use orchestrator::{
    errors::OrchestratorError,
    network::{node::NetworkNode, Network},
    AddCollatorOptions, AddNodeOptions, Orchestrator,
};

// Helpers used for interact with the network
pub mod tx_helper {
    pub use orchestrator::{
        network::chain_upgrade::ChainUpgrade, shared::types::RuntimeUpgradeOptions,
    };
}

use provider::{DockerProvider, KubernetesProvider, NativeProvider};
pub use support::fs::local::LocalFileSystem;

pub mod environment;
pub const PROVIDERS: [&str; 3] = ["k8s", "native", "docker"];

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
    async fn spawn_docker(self) -> Result<Network<LocalFileSystem>, OrchestratorError>;
}

#[async_trait]
impl NetworkConfigExt for NetworkConfig {
    async fn spawn_native(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = NativeProvider::new(filesystem.clone());
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.spawn(self).await
    }

    async fn spawn_k8s(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = KubernetesProvider::new(filesystem.clone()).await;
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.spawn(self).await
    }

    async fn spawn_docker(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = DockerProvider::new(filesystem.clone()).await;
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.spawn(self).await
    }
}
