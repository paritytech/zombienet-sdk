mod errors;
use std::time::Duration;

use configuration::NetworkConfig;
use errors::OrchestratorError;
use provider::Provider;
use support::fs::FileSystem;
use tokio::time::timeout;

pub struct Orchestrator<T, P>
where
    T: FileSystem + Sync + Send,
    P: Provider,
{
    filesystem: T,
    provider: P,
}

impl<T, P> Orchestrator<T, P>
where
    T: FileSystem + Sync + Send,
    P: Provider,
{
    pub fn new(filesystem: T, provider: P) -> Self {
        Self {
            filesystem,
            provider,
        }
    }

    pub async fn spawn(&self, network_config: NetworkConfig) -> Result<(), OrchestratorError> {
        let global_timeout = network_config.global_settings().network_spawn_timeout();
        let r = timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_config),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout))?;
        r
    }

    async fn spawn_inner(&self, network_config: NetworkConfig) -> Result<(), OrchestratorError> {
        // ensure valid provider access (mostly checking the deps/creds)
        // `tmp` dir is already part of the `provider` (do we need here?)
        // create_namespace
        // setup_static
        // chain_spec (relay)
        // parachain -> generate artifacts
        // loop to add to genesis IFF not raw and needs to
        // get chain-spec raw
        // spawn bootnode / first node
        // spawn in batch the bodes from
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
