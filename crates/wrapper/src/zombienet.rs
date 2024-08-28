use std::marker::PhantomData;

struct EmptyState;
struct NodeOnly;
struct ParachainOnly;
struct NodeAndParachain;

// Object representing the final build object
pub struct Zombienet {
	nodes: Option<Vec<ZombieNode>>,
	parachain: Option<ZombieParachain<IdAndCollator>>,
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
impl<S> OptionalMethods for Zombienet<S> {
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

impl Zombienet<EmptyState> {
		pub fn new() -> Self {
	      Self {
		        nodes: None,
						parachain: None,
	          /* other fields */,
	          state: std::marker::PhantomData,
	      }
    }

    pub fn set_nodes(self, nodes: Vec<ZombieNode>) -> Zombienet<NodeOnly> {
		    // nodes can't be empty!

		    Zombienet {
				    nodes: Some(nodes),
				    parachain: self.parachain,
				    /* other fields */
				}
    }

    pub fn set_parachain(self, parachain: ZombieParachain<IdAndCollator>) -> Zombienet<ParachainOnly> {
    		Zombienet {
				    nodes: self.nodes,
				    parachain: Some(parachain),
				    /* other fields */
				}
    }
}

impl ZombieParachain<NodeOnly> {
		pub fn set_parachain(self, collators: Vec<ZombieCollator>) -> Zombienet<NodeAndParachain> {
    		Zombienet {
				    nodes: self.nodes,
				    parachain: Some(parachain),
				    /* other fields */
				}
    }
}

impl Zombienet<ParachainOnly> {
    pub fn set_nodes(self, nodes: Vec<ZombieNode>) -> Zombienet<NodeAndParachain> {
		    // nodes can't be empty!

		    Zombienet {
				    nodes: Some(nodes),
				    parachain: self.parachain,
				    /* other fields */
				}
		}
}


impl Zombienet<NodeAndParachain> {
		// spawn() is available only after both `nodes` and `parachain` are set
		pub fn spawn(self) -> Result<Network<LocalFileSystem>, OrchestratorError> {}
}

