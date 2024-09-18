pub struct Zombienet {
    nodes: Vec<ZombieNode>,
    parachain: ZombieParachain,
    // other fields
}

impl Zombienet {
    /// the first parameter is the list of nodes: `Vec<ZombieNode>`
    /// the second parameter is the parachain: `ZombieParachain`
    pub fn new(nodes: Vec<ZombieNode>, parachains: ZombieParachain) -> Self {
        Self {
            nodes,
            parachain,
            // other fields
        }
    }

    // consume and return a new object with the modifications
    // to allow user to chain operations
    pub fn otherMethod(mut self) -> Self {}

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
