pub struct ZombieCollator {
    name: String,
    // other fields
}

impl ZombieCollator {
    // takes the name of the collator as the parameter
    pub fn new(name: String) -> Self {
        // name can't be empty!

        ZombieCollator {
            name, // other fields with default initialization
        }
    }

    // consume and return a new object with the modifications
    // to allow user to chain operations
    pub fn otherMethod(mut self) -> Self {}
}
