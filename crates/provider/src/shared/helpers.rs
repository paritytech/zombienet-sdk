use std::env;

/// Check if we are running in `CI` by checking the 'RUN_IN_CI' env var
pub fn running_in_ci() -> bool {
    env::var("RUN_IN_CI").unwrap_or_default() == "1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_runing_in_ci_env_var() {
        assert!(!running_in_ci());
        // now set the env var
        env::set_var("RUN_IN_CI", "1");
        assert!(running_in_ci());
        // reset
        env::set_var("RUN_IN_CI", "");
    }
}
