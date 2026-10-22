use std::path::PathBuf;

use async_trait::async_trait;
pub use configuration::{
    json_patch,
    types::{Arg, AssetLocation},
    Buildable, CustomProcess, CustomProcessBuilder, Dependency, DependencyCondition, EnvValue,
    GlobalSettings, GlobalSettingsBuilder, NamedPort, NetworkConfig, NetworkConfigBuilder,
    ProcessEnvVar, ReadyCheck, ReadyTarget, RegistrationStrategy, TransferedFile,
};
pub use orchestrator::{
    errors::OrchestratorError,
    network::{
        jamchain::Jamchain,
        node::{
            CustomProcessNode, CustomProcessState, JamNetworkNode, NetworkNode, NodeKind,
            ProcessPort, SpawnedNode,
        },
        Network,
    },
    sc_chain_spec, AddCollatorOptions, AddNodeOptions, Orchestrator,
};
pub use provider::types::{ExecutionResult, RunScriptOptions};

/// Generator that could be handy from outside
pub mod generators {
    pub use orchestrator::generators::{
        core_assignment, errors, generate_node_bootnode_addr, generate_node_identity,
        generate_node_keys,
    };
}

/// JAM: the genesis model of a spec built in process, and the blobs zombienet embeds.
pub mod jam {
    pub use configuration::{
        JamAmount, JamAuthorizer, JamBalance, JamBytes, JamCoreIndex, JamGenesisConfig,
        JamGenesisConfigBuilder, JamGenesisPrivileges, JamGenesisService, JamGenesisServiceBuilder,
        JamServiceId, JAM_BOOTSTRAP_SERVICE_ID,
    };
    pub use orchestrator::generators::jam_blobs::{
        authorizer_hash, embedded_bootstrap_service, null_authorizer_hash, NULL_AUTHORIZER_BLOB,
        NULL_AUTHORIZER_HASH,
    };
    /// The embedded bootstrap service (with the `jam` feature).
    #[cfg(feature = "jam")]
    pub use orchestrator::generators::jam_blobs::{
        BOOTSTRAP_SERVICE_BLOB, BOOTSTRAP_SERVICE_HASH, BOOTSTRAP_SERVICE_SOURCE,
    };
}

/// Helpers used for interact with the network
pub mod tx_helper {
    pub use orchestrator::{
        network::chain_upgrade::ChainUpgrade,
        shared::types::RuntimeUpgradeOptions,
        tx_helper::{parachain, runtime_upgrade},
    };
}

use provider::{DockerProvider, KubernetesProvider, NativeProvider};
pub use support::fs::local::LocalFileSystem;

pub mod environment;
pub mod snapshot;
pub use snapshot::{Bundle, BundleBuilder, SnapshotManifest};
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
    /// Spawns a network using the k8s provider into `namespace`, which must already
    /// exist. It is left in place if spawning fails; `Network::destroy` deletes it.
    async fn spawn_k8s_in_namespace(
        self,
        namespace: &str,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError>;
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

    /// Attaches to a running live network using the k8s provider, from in-memory
    /// `zombie.json` content. Its `local_base_dir` is used as-is.
    ///
    /// # Example:
    /// ```rust
    /// # use zombienet_sdk::{AttachToLive, AttachToLiveNetwork};
    /// # async fn example(zombie_json: serde_json::Value) -> Result<(), zombienet_sdk::OrchestratorError> {
    /// let network = AttachToLiveNetwork::attach_k8s_from_json(zombie_json).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn attach_k8s_from_json(
        zombie_json: serde_json::Value,
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

    async fn spawn_k8s_in_namespace(
        self,
        namespace: &str,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = KubernetesProvider::new_in_namespace(filesystem.clone(), namespace).await;
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

    async fn attach_k8s_from_json(
        zombie_json: serde_json::Value,
    ) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let filesystem = LocalFileSystem;
        let provider = KubernetesProvider::new(filesystem.clone()).await;
        let orchestrator = Orchestrator::new(filesystem, provider);
        orchestrator.attach_to_live_from_json(&zombie_json).await
    }
}
