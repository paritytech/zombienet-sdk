// TODO(Javier): Remove when we implement the logic in the orchestrator to spawn with the provider.
#![allow(dead_code)]

mod errors;
mod generators;
mod network_spec;
mod shared;

use std::time::Duration;

use configuration::NetworkConfig;
use errors::OrchestratorError;
use network_spec::NetworkSpec;
use provider::Provider;
use support::fs::FileSystem;
use tokio::time::timeout;
// use rand::Rng;

pub struct Orchestrator<T, P>
where
    T: FileSystem + Sync + Send,
    P: Provider,
{
    filesystem: T,
    provider_ctor: fn(&str, &str, T) -> P,
}

impl<T, P> Orchestrator<T, P>
where
    T: FileSystem + Sync + Send + Clone,
    P: Provider,
{
    pub fn init(filesystem: T, provider_ctor: fn(&str, &str, T) -> P) -> Self {
        Self {
            filesystem,
            provider_ctor,
        }
    }

    pub async fn spawn(&self, network_config: NetworkConfig) -> Result<(), OrchestratorError> {
        let global_timeout = network_config.global_settings().network_spawn_timeout();
        let network_spec = NetworkSpec::from_config(&network_config).await?;

        timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_spec),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout))?
    }

    async fn spawn_inner(&self, _network_spec: NetworkSpec) -> Result<(), OrchestratorError> {
        // main dirver for spawn the network

        // init provider & create namespace

        // static setup

        // Creta chain-spec for relaychain

        // Create parachain artifacts (chain-spec, wasm, state)

        // Customize relaychain

        // spawn first node of relay-chain and any parachain

        // spawn the rest of the nodes (in batches)

        // add-ons (introspector/tracing/etc)

        // verify nodes (clean metrics cache?)

        // write zombie.json state file

        // return `Network` instance
        Ok(())
    }
}
