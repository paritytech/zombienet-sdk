use crate::types::{Chain, Command, Duration};

pub(crate) fn is_true(value: &bool) -> bool {
    *value
}

pub(crate) fn is_false(value: &bool) -> bool {
    !(*value)
}

pub(crate) fn default_as_true() -> bool {
    true
}

pub(crate) fn default_as_false() -> bool {
    false
}

pub(crate) fn default_initial_balance() -> crate::types::U128 {
    2_000_000_000_000.into()
}

/// Default timeout for spawning a node (10mins)
pub(crate) fn default_node_spawn_timeout() -> Duration {
    600
}

/// Default timeout for spawning the whole network (1hr)
pub(crate) fn default_timeout() -> Duration {
    3600
}

pub(crate) fn default_command_polkadot() -> Option<Command> {
    TryInto::<Command>::try_into("polkadot").ok()
}

pub(crate) fn default_relaychain_chain() -> Chain {
    TryInto::<Chain>::try_into("rococo-local").expect("'rococo-local' should be a valid chain")
}
