pub const VALID_REGEX: &str = "regex should be valid ";
pub const BORROWABLE: &str = "must be borrowable as mutable ";
pub const RELAY_NOT_NONE: &str = "typestate should ensure the relaychain isn't None at this point ";
pub const SHOULD_COMPILE: &str = "should compile with success ";
pub const INFAILABLE: &str = "infaillible ";
pub const NO_ERR_DEF_BUILDER: &str = "should have no errors for default builder ";
pub const RW_FAILED: &str = "should be able to read/write - failed ";
pub const DEFAULT_TYPESTATE: &str = "'default' overriding should be ensured by typestate ";
pub const VALIDATION_CHECK: &str = "validation failed ";

pub const PREFIX_CANT_BE_NONE: &str = "name prefix can't be None if a value exists ";

pub const GRAPH_CONTAINS_NAME: &str =
    "graph contains node name; we initialize it with all node names";
pub const GRAPH_CONTAINS_DEP: &str = "graph contains dep_name; we filter out deps not contained in by_name and populate the graph with all nodes";
pub const INDEGREE_CONTAINS_NAME: &str =
    "indegree contains node name; we initialize it with all node names";
pub const QUEUE_NOT_EMPTY: &str = "queue is not empty; we're looping over its length";

pub const THIS_IS_A_BUG: &str =
    "- this is a bug please report it: https://github.com/paritytech/zombienet-sdk/issues";

/// environment variable which can be used to override node spawn timeout
pub const ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS: &str = "ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS";
