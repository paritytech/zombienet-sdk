use crate::types::{Command, Duration};

pub(crate) fn is_true(value: &bool) -> bool {
    *value
}

pub(crate) fn is_false(value: &bool) -> bool {
    !(*value)
}

pub(crate) fn default_as_true() -> bool {
    true
}

pub(crate) fn default_initial_balance() -> crate::types::U128 {
    2_000_000_000_000.into()
}

pub(crate) fn default_node_spawn_timeout() -> Duration {
    300
}

pub(crate) fn default_command_polkadot() -> Option<Command> {
    TryInto::<Command>::try_into("polkadot").ok()
}
