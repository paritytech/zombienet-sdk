use std::{collections::HashMap, env};

use configuration::shared::constants::VALID_REGEX;
use regex::{Captures, Regex};

pub(crate) fn apply_replacements(text: &str, replacements: &HashMap<&str, &str>) -> String {
    let re = Regex::new(r#"\{\{([a-zA-Z0-9_]*)\}\}"#).unwrap_or_else(|_| panic!("{} {}",
        VALID_REGEX,
        configuration::shared::constants::THIS_IS_A_BUG));

    let augmented_text = re.replace_all(text, |caps: &Captures| {
        if let Some(replacements_value) = replacements.get(&caps[1]) {
            replacements_value.to_string()
        } else {
            caps[0].to_string()
        }
    });

    augmented_text.to_string()
}

/// Check if we are running in `CI` by checking the 'RUN_IN_CI' env var
pub fn running_in_ci() -> bool {
    env::var("RUN_IN_CI").unwrap_or_default() == "1"
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn replace_should_works() {
        let text = "some {{namespace}}";
        let mut replacements = HashMap::new();
        replacements.insert("namespace", "demo-123");
        let res = apply_replacements(text, &replacements);
        assert_eq!("some demo-123".to_string(), res);
    }

    #[test]
    fn replace_multiple_should_works() {
        let text = r#"some {{namespace}}
        other is {{other}}"#;
        let augmented_text = r#"some demo-123
        other is other-123"#;

        let mut replacements = HashMap::new();
        replacements.insert("namespace", "demo-123");
        replacements.insert("other", "other-123");
        let res = apply_replacements(text, &replacements);
        assert_eq!(augmented_text, res);
    }

    #[test]
    fn replace_multiple_with_missing_should_works() {
        let text = r#"some {{namespace}}
        other is {{other}}"#;
        let augmented_text = r#"some demo-123
        other is {{other}}"#;

        let mut replacements = HashMap::new();
        replacements.insert("namespace", "demo-123");

        let res = apply_replacements(text, &replacements);
        assert_eq!(augmented_text, res);
    }

    #[test]
    fn replace_without_replacement_should_leave_text_unchanged() {
        let text = "some {{namespace}}";
        let mut replacements = HashMap::new();
        replacements.insert("other", "demo-123");
        let res = apply_replacements(text, &replacements);
        assert_eq!(text.to_string(), res);
    }

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
