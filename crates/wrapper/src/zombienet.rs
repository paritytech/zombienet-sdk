use configuration::{
    GlobalSettingsBuilder, HrmpChannelConfigBuilder, HrmpInitialState, NetworkConfigBuilder,
    RelaychainConfigBuilder, RelaychainInitialState, WithAtLeastOneNode, WithRecipient,
    WithRelaychain,
};

use crate::{zombie_node::ZombieNode, zombie_parachain::ZombieParachain};

pub struct ZombienetBuilder {
    parachain: ZombieParachain,
    network: NetworkConfigBuilder<WithRelaychain>,
}

impl ZombienetBuilder {
    /// the first parameter is the list of nodes: `Vec<ZombieNode>`
    /// the second parameter is the parachain: `ZombieParachain`
    /// relaychain and node settings are set to default,
    /// use `new_custom` to set a custom relaychain and custom node settings.
    pub fn new(nodes: Vec<ZombieNode>, parachain: ZombieParachain) -> Self {
        let network = NetworkConfigBuilder::new().with_relaychain(|relaychain| {
            let mut relayhcain_with_node = relaychain
                .with_chain("rococo-local")
                .with_node(|node| node.with_name(nodes.first().unwrap().name()));

            for node in nodes.iter().skip(1) {
                relayhcain_with_node = relayhcain_with_node
                    .with_node(|node_builder| node_builder.with_name(node.name()));
            }
            relayhcain_with_node
        });

        Self { network, parachain }
    }

    /// allows you to fully configure the relaychain,
    /// the closure `f` you provide has to initialize nodes as well
    /// use `new` if you want to go with the default relaychain
    pub fn new_custom(
        parachain: ZombieParachain,
        f: impl FnOnce(
            RelaychainConfigBuilder<RelaychainInitialState>,
        ) -> RelaychainConfigBuilder<WithAtLeastOneNode>,
    ) -> Self {
        let network = NetworkConfigBuilder::new().with_relaychain(f);

        Self { network, parachain }
    }

    /// optional method for setting hrmp channel, hrmp is not set by default
    pub fn set_hrmp_channel(
        mut self,
        f: impl FnOnce(
            HrmpChannelConfigBuilder<HrmpInitialState>,
        ) -> HrmpChannelConfigBuilder<WithRecipient>,
    ) -> Self {
        self.network = self.network.with_hrmp_channel(f);
        self
    }

    pub fn set_global_settings(
        mut self,
        f: impl FnOnce(GlobalSettingsBuilder) -> GlobalSettingsBuilder,
    ) -> Self {
        self.network = self.network.with_global_settings(f);
        self
    }

    // spawn() is available only after both `nodes` and `parachain` are set
    pub fn spawn(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {
        if self.nodes.is_none() || self.parachain.is_none() {
            return Err(OrchestratorError::InvalidConfig(
                "`nodes` or `parachain` field is not set for the network.",
            ));
        }

        // rest of the body
    }
}
