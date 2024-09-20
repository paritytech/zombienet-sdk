pub struct ZombieNode {
    name: String,
    // other fields
}

impl ZombieNode {
    // takes the name of the node as the parameter
    pub fn new(name: String) -> Self {
        // name can't be empty!

        ZombieNode {
            name, // other fields with default initialization
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    // consume and return a new object with the modifications
    // to allow user to chain operations
    pub fn otherMethod(mut self) -> Self {
        self
    }
}
