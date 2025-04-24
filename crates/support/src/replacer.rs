use std::collections::HashMap;
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use tracing::{trace, warn};

use crate::constants::{THIS_IS_A_BUG, SHOULD_COMPILE};

lazy_static! {
    static ref RE: Regex = Regex::new(r#"\{\{([a-zA-Z0-9_]*)\}\}"#)
        .expect(&format!("{}, {}", SHOULD_COMPILE, THIS_IS_A_BUG));

    static ref TOKEN_PLACEHOLDER: Regex = Regex::new(r#"\{\{ZOMBIE:(.*?):(.*?)\}\}"#)
        .expect(&format!("{}, {}", SHOULD_COMPILE, THIS_IS_A_BUG));

    static ref PLACEHOLDER_COMPAT: HashMap<&'static str, &'static str> = {
            let mut m = HashMap::new();
            m.insert("multiAddress", "multiaddr");
            m.insert("wsUri", "ws_uri");

            m
        };

}

pub fn apply_replacements(text: &str, replacements: &HashMap<&str, &str>) -> String {
    let augmented_text = RE.replace_all(text, |caps: &Captures| {
        if let Some(replacements_value) = replacements.get(&caps[1]) {
            replacements_value.to_string()
        } else {
            caps[0].to_string()
        }
    });

    augmented_text.to_string()
}

pub fn apply_env_replacements(text: &str) -> String {
    let augmented_text = RE.replace_all(text, |caps: &Captures| {
        if let Ok(replacements_value) = std::env::var(&caps[1]) {
            replacements_value
        } else {
            caps[0].to_string()
        }
    });

    augmented_text.to_string()
}

pub fn apply_running_network_replacements(text: &str, network: &serde_json::Value) -> String {
    let augmented_text = TOKEN_PLACEHOLDER.replace_all(text, |caps: &Captures| {
        trace!("appling replacements for caps: {caps:#?}");
        if let Some(node) = network.get(&caps[1]) {
            trace!("caps1 {} - node: {node}", &caps[1]);
            let field = *PLACEHOLDER_COMPAT.get(&caps[2]).unwrap_or(&&caps[2]);
            if let Some(val) = node.get(&field) {
                trace!("caps2 {} - node: {node}", field);
                val.as_str().unwrap_or("Invalid string").to_string()
            } else {
                warn!("⚠️ The node with name {} doesn't have the value {} in context", &caps[1], &caps[2]);
                caps[0].to_string()
            }
        } else {
            warn!("⚠️ No node with name {} in context", &caps[1]);
            caps[0].to_string()
        }
    });

    augmented_text.to_string()
}
#[cfg(test)]
mod tests {
    use serde_json::json;

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
    fn replace_env_should_works() {
        let text = "some {{namespace}}";
        std::env::set_var("namespace", "demo-123");
        // let mut replacements = HashMap::new();
        // replacements.insert("namespace", "demo-123");
        let res = apply_env_replacements(text);
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
    fn replace_running_network_should_work() {
        let network = json!({
            "alice" : {
                "multiaddr": "some/demo/127.0.0.1"
            }
        });

        let res = apply_running_network_replacements("{{ZOMBIE:alice:multiaddr}}", &network);
        assert_eq!(res.as_str(), "some/demo/127.0.0.1");
    }

    #[test]
    fn replace_running_network_with_compat_should_work() {
        let network = json!({
            "alice" : {
                "multiaddr": "some/demo/127.0.0.1"
            }
        });

        let res = apply_running_network_replacements("{{ZOMBIE:alice:multiAddress}}", &network);
        assert_eq!(res.as_str(), "some/demo/127.0.0.1");
    }

    #[test]
    fn replace_running_network_with_missing_field_should_not_replace_nothing() {
        let network = json!({
            "alice" : {
                "multiaddr": "some/demo/127.0.0.1"
            }
        });

        let res = apply_running_network_replacements("{{ZOMBIE:alice:someField}}", &network);
        assert_eq!(res.as_str(), "{{ZOMBIE:alice:someField}}");
    }
}
