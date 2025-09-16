use std::env;

use support::constants::ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS;

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
    env::var(ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(600)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_node_spawn_timeout_works_when_env_is_set() {
        env::set_var(ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS, "123");
        assert_eq!(default_node_spawn_timeout(), 123);
    }

    #[test]
    fn default_node_spawn_timeout_falls_back_to_default_when_env_is_not_set() {
        assert_eq!(default_node_spawn_timeout(), 600);
    }

    #[test]
    fn default_node_spawn_timeout_falls_back_to_default_when_env_is_not_parsable() {
        env::set_var(ZOMBIE_NODE_SPAWN_TIMEOUT_SECONDS, "NOT_A_NUMBER");
        assert_eq!(default_node_spawn_timeout(), 600);
    }
}
