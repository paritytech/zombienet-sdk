use async_trait::async_trait;
pub use configuration::{NetworkConfig, NetworkConfigBuilder, RegistrationStrategy};
pub use orchestrator::{errors::OrchestratorError, network::Network, AddNodeOpts, Orchestrator};
use provider::NativeProvider;
use support::{fs::local::LocalFileSystem, process::os::OsProcessManager};

#[async_trait]
pub trait Spawner {
    /// Spawns a network using the native provider.
    async fn spawn_native(self) -> Result<Network<LocalFileSystem>, OrchestratorError>;
}

#[async_trait]
impl Spawner for NetworkConfig {
    async fn spawn_native(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        let provider = NativeProvider::new(LocalFileSystem {}, OsProcessManager {});
        let orchestrator = Orchestrator::new(LocalFileSystem {}, provider);
        orchestrator.spawn(self).await
    }
}
