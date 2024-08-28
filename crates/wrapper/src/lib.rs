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

// ------

pub struct ZombieCollator {
    name: String,
    // other fields
}

impl ZombieNode {
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


use std::marker::PhantomData;

struct EmptyState;
struct IdOnly;
struct CollatorOnly;
struct IdAndCollator;

// Object representing the final build object
pub struct ZombieParachain {
	id: Option<u8>,
	collators: Option<Vec<ZombieCollator>>,
	/* other fields	*/
}

// Trait for optional methods
// any methods related to other fields can go in here,
// these methods will be callable in any state of `ZombieParachain`
pub trait OptionalMethods {
    pub fn c(self, c: String) -> Self;
    pub fn d(self, d: String) -> Self;
    pub fn e(self, e: String) -> Self;
}

// Implement optional methods for all states
impl<S> OptionalMethods for ZombieParachain<S> {
    pub fn c(mut self, c: String) -> Self {
        self.c = Some(c);
        self
    }

    pub fn d(mut self, d: String) -> Self {
        self.d = Some(d);
        self
    }

    pub fn e(mut self, e: String) -> Self {
        self.e = Some(e);
        self
    }
}

impl ZombieParachain<EmptyState> {
		pub fn new() -> Self {
	      Self {
	          id: None,
	          collators: None,
	          /* other fields */,
	          state: std::marker::PhantomData,
	      }
    }

    pub fn set_id(self, id: u8) -> ZombieParachain<IdOnly> {
		    ZombieParachain {
				    id: Some(id),
				    collators: self.collators,
				    /* other fields */
				}
    }

    pub fn set_collators(self, collators: Vec<ZombieCollator>) -> ZombieParachain<CollatorOnly> {
		    // collators can't be empty!

    		ZombieParachain {
				    id: self.id,
				    collators: Some(collators),
				    /* other fields */
				}
    }
}

impl ZombieParachain<IdOnly> {
		pub fn set_collators(self, collators: Vec<ZombieCollator>) -> ZombieParachain<IdAndCollator> {
				// collators can't be empty!

    		ZombieParachain {
				    id: self.id,
				    collators: Some(collators),
				    /* other fields */
				}
    }
}

impl ZombieParachain<CollatorOnly> {
    pub fn set_id(self, id: u8) -> ZombieParachain<IdAndCollator> {
		    ZombieParachain {
				    id: Some(id),
				    collators: self.collators,
				    /* other fields */
				}
    }
}

// `ZombieParachain<IdAndCollator>` will be used by `Zombienet` struct.

