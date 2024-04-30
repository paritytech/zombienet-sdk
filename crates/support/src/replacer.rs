use std::collections::HashMap;

use regex::{Captures, Regex};

use crate::constants::{THIS_IS_A_BUG, VALID_REGEX};

pub fn apply_replacements(text: &str, replacements: &HashMap<&str, &str>) -> String {
    let re = Regex::new(r#"\{\{([a-zA-Z0-9_]*)\}\}"#)
        .unwrap_or_else(|_| panic!("{} {}", VALID_REGEX, THIS_IS_A_BUG));

    let augmented_text = re.replace_all(text, |caps: &Captures| {
        if let Some(replacements_value) = replacements.get(&caps[1]) {
            replacements_value.to_string()
        } else {
            caps[0].to_string()
        }
    });

    augmented_text.to_string()
}

#[cfg(test)]
mod tests {
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
}
