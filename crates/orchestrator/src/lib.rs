mod errors;
mod chain_spec;
mod network_spec;
mod generators;

use std::time::Duration;
use chain_spec::ChainSpec;
use configuration::NetworkConfig;
use errors::OrchestratorError;
use provider::Provider;
use support::fs::FileSystem;
use tokio::time::timeout;
use rand::Rng;

pub struct Orchestrator<T, P>
where
    T: FileSystem + Sync + Send,
    P: Provider,
{
    filesystem: T,
    provider_ctor: fn(&str, &str,T) -> P,
}

impl<T, P> Orchestrator<T, P>
where
    T: FileSystem + Sync + Send + Clone,
    P: Provider,
{
    pub fn init(filesystem: T, provider_ctor: fn(&str, &str,T) -> P) -> Self {
        Self {
            filesystem,
            provider_ctor
        }
    }


    pub async fn spawn(&self, network_config: NetworkConfig) -> Result<(), OrchestratorError> {
        let global_timeout = 1000;// network_config.global_settings().network_spawn_timeout();
        let r = timeout(
            Duration::from_secs(global_timeout.into()),
            self.spawn_inner(network_config),
        )
        .await
        .map_err(|_| OrchestratorError::GlobalTimeOut(global_timeout))?;
        r
    }

    async fn spawn_inner(&self, network_config: NetworkConfig) -> Result<(), OrchestratorError> {
        // Create NetworkSpec

        // Create namespace
        let namespace = format!("zombie-{:x?}", rand::thread_rng().gen::<[u8; 32]>());
        let system_tmp_dir = std::env::temp_dir();
        let working_dir = format!("{}/{}", system_tmp_dir.display(), namespace);

        // init provider
        let mut provider = (self.provider_ctor)(&namespace, &working_dir, self.filesystem.clone());
        // TODO: ensure valid provider access (mostly checking the deps/creds)

        // create_namespace
        provider.create_namespace().await?;

        // setup_static
        provider.static_setup().await?;


        // chain_spec (relay)
        // Get the default command or use the command from the first node in the
        // relaychain to build the spec.
        let build_spec_cmd = if let Some(cmd) = network_config.relaychain().default_command() {
            Some(cmd)
        } else {
            if let Some(node) = network_config.relaychain().nodes().first() {
                node.command()
            } else {
                None
            }
        };

        let Some(build_spec_cmd) = build_spec_cmd else {
            return Err(OrchestratorError::InvalidConnfig)
        };

        // let build_spec_image = if let Some(image) = network_config.relaychain().default_image() {
        //     Some(image)
        // } else {

        // }

        let mut relaychain_spec = ChainSpec::new(network_config.relaychain().chain().as_str(), build_spec_cmd.as_str());
        let _r = relaychain_spec.build(&provider).await;
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
    use configuration::NetworkConfigBuilder;
    use provider::NativeProvider;
    use support::fs::mock::MockFilesystem;

    use super::*;

    #[tokio::test]
    async fn test_smoke() {
        let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_node(|node| {
                    node.with_name("alice").with_command("polkadot")
            })
        })
        .build().unwrap();

        let orc = Orchestrator::init(MockFilesystem::new(), |a, b, c| {
            NativeProvider::new(a, b, c)
        });

        let r = orc.spawn(config).await.unwrap();
        println!("{:?}", r);

        // let native_provider = NativeProvider::new("something", "./", "/tmp", MockFilesystem::new());

        // let mut some = native_provider.run_command(
        //     vec!["ls".into(), "ls".into()],
        //     NativeRunCommandOptions::default(),
        // );

        // assert!(some.await.is_err());

        // some = native_provider.run_command(
        //     vec!["ls".into(), "ls".into()],
        //     NativeRunCommandOptions {
        //         is_failure_allowed: true,
        //     },
        // );

        // assert!(some.await.is_ok());
    }
}
