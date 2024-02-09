use std::collections::HashMap;

use configuration::shared::constants::VALID_REGEX;
use regex::{Captures, Regex};

pub(crate) fn apply_replacements(text: &str, replacements: &HashMap<&str, &str>) -> String {
    let re = Regex::new(r#"\{\{([a-zA-Z0-9_]*)\}\}"#).expect(&format!(
        "{} {}",
        VALID_REGEX,
        configuration::shared::constants::THIS_IS_A_BUG
    ));

    let augmented_text = re.replace_all(text, |caps: &Captures| {
        if let Some(replacements_value) = replacements.get(&caps[1]) {
            replacements_value.to_string()
        } else {
            format!("{}", &caps[0])
        }
    });

    augmented_text.to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn replace_should_works() {
        let text = "some {{namespace}}";
        let mut replacements = HashMap::new();
        replacements.insert("namespace".into(), "demo-123".into());
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
        replacements.insert("namespace".into(), "demo-123".into());
        replacements.insert("other".into(), "other-123".into());
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
        replacements.insert("namespace".into(), "demo-123".into());

        let res = apply_replacements(text, &replacements);
        assert_eq!(augmented_text, res);
    }

    #[test]
    fn replace_without_replacement_should_leave_text_unchanged() {
        let text = "some {{namespace}}";
        let mut replacements = HashMap::new();
        replacements.insert("other".into(), "demo-123".into());
        let res = apply_replacements(text, &replacements);
        assert_eq!(text.to_string(), res);
    }
}
