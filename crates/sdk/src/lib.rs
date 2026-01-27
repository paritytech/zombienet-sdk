use std::path::PathBuf;

use async_trait::async_trait;
pub use configuration::{
    GlobalSettings, GlobalSettingsBuilder, NetworkConfig, NetworkConfigBuilder,
    RegistrationStrategy, WithRelaychain, CustomProcess, CustomProcessBuilder
};
pub use orchestrator::{
    errors::OrchestratorError,
    network::{node::NetworkNode, Network},
    sc_chain_spec, AddCollatorOptions, AddNodeOptions, Orchestrator,
};
pub use provider::types::{ExecutionResult, RunScriptOptions};

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

// re-export subxt
pub use subxt;
pub use subxt_signer;

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
pub trait AttachToLive {
    /// Attaches to a running live network using the native, docker or k8s provider.
    ///
    /// # Example:
    /// ```rust
    /// # use zombienet_sdk::{AttachToLive, AttachToLiveNetwork};
    /// # use std::path::PathBuf;
    /// # async fn example() -> Result<(), zombienet_sdk::OrchestratorError> {
    /// let zombie_json_path = PathBuf::from("some/path/zombie.json");
    /// let network = AttachToLiveNetwork::attach_native(zombie_json_path).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn attach_native(
        zombie_json_path: PathBuf,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError>;
    async fn attach_k8s(
        zombie_json_path: PathBuf,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError>;
    async fn attach_docker(
        zombie_json_path: PathBuf,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError>;
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

pub struct AttachToLiveNetwork;

#[async_trait]
impl AttachToLive for AttachToLiveNetwork {
    async fn attach_native(
        zombie_json_path: PathBuf,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = NativeProvider::new(filesystem.clone());
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.attach_to_live(zombie_json_path.as_ref()).await
    }

    async fn attach_k8s(
        zombie_json_path: PathBuf,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = KubernetesProvider::new(filesystem.clone()).await;
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.attach_to_live(zombie_json_path.as_ref()).await
    }

    async fn attach_docker(
        zombie_json_path: PathBuf,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = DockerProvider::new(filesystem.clone()).await;
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.attach_to_live(zombie_json_path.as_ref()).await
    }
}
