pub struct ZombieNode {
    name: String,
    // other fields
}

impl ZombieNode {
    pub fn new(name: String) -> Self {
        // name can't be empty!

        ZombieNode {
            name, // other fields with default initialization
        }
    }

    // consume and return a new object with the modifications
    // to allow user to chain operations
    pub fn otherMethod(mut self) -> Self {}
}
