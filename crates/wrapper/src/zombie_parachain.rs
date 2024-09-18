pub struct ZombieParachain {
    id: u8,
    collators: Vec<ZombieCollator>,
    // other fields
}

impl ZombieParachain {
    /// the first parameter is the ID of the parachain: `u8`,
    /// the second parameter is the list of collators: `Vec<ZombieCollator>`
    pub fn new(id: u8, collators: Vec<ZombieCollator>) -> Self {
        Self {
            id,
            collators,
            // other fields
        }
    }

    // consume and return a new object with the modifications
    // to allow user to chain operations
    pub fn otherMethod(mut self) -> Self {}
}
