use async_trait::async_trait;
#[deprecated]
pub use configuration::RegistrationStrategy;
// Top level access to sdk
pub use configuration::{NetworkConfig, NetworkConfigBuilder};
// Orchestrator (TODO: export using orchestrator scope)
pub use orchestrator::{
    errors::OrchestratorError,
    network::{node::NetworkNode, Network},
    AddCollatorOptions, AddNodeOptions, Orchestrator,
};

// Providers list
pub const PROVIDERS: [&str; 3] = ["k8s", "native", "docker"];
// Allow to create single provider from sdk
pub mod provider {
    pub use provider::{
        DockerProvider, DynNamespace, DynNode, DynProvider, KubernetesProvider, NativeProvider,
    };
}

#[cfg(feature = "pjs")]
pub use orchestrator::pjs_helper::PjsResult;

// Helpers used for interact with the network
pub mod tx_helper {
    pub use orchestrator::{
        // Runtime upgrade call
        network::chain_upgrade::ChainUpgrade,
        shared::types::RuntimeUpgradeOptions,
    };
}

// Shared type from other crates
pub mod shared {
    pub mod configuration {
        // Allow to construct config types
        pub use configuration::shared::types::{Arg, AssetLocation};
        pub use configuration::RegistrationStrategy; // TODO: move to shared
    }

    pub mod provider {
        pub use provider::shared::types::{
            ExecutionResult, RunCommandOptions, RunScriptOptions, SpawnNodeOptions,
        };
    }

    pub mod support {
        pub use support::net;
    }
}

// LocalFileSystem used by orchestrator/provider
pub use support::fs::local::LocalFileSystem;

/// Environment helper
pub mod environment;

use provider::{DockerProvider, KubernetesProvider, NativeProvider};
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
