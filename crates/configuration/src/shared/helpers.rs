use std::{cell::RefCell, collections::HashSet, rc::Rc};

use support::constants::{BORROWABLE, THIS_IS_A_BUG};
use tracing::warn;

use super::{
    errors::ValidationError,
    types::{ParaId, Port, ValidationContext},
};

pub fn merge_errors(errors: Vec<anyhow::Error>, new_error: anyhow::Error) -> Vec<anyhow::Error> {
    let mut errors = errors;
    errors.push(new_error);

    errors
}

pub fn merge_errors_vecs(
    errors: Vec<anyhow::Error>,
    new_errors: Vec<anyhow::Error>,
) -> Vec<anyhow::Error> {
    let mut errors = errors;

    for new_error in new_errors.into_iter() {
        errors.push(new_error);
    }

    errors
}

/// Generates a unique name from a base name and the names already present in a
/// [`ValidationContext`].
///
/// Uses [`generate_unique_node_name_from_names()`] internally to ensure uniqueness.
/// Logs a warning if the generated name differs from the original due to duplicates.
pub fn generate_unique_node_name(
    node_name: impl Into<String>,
    validation_context: Rc<RefCell<ValidationContext>>,
) -> String {
    let mut context = validation_context
        .try_borrow_mut()
        .expect(&format!("{BORROWABLE}, {THIS_IS_A_BUG}"));

    generate_unique_node_name_from_names(node_name, &mut context.used_nodes_names)
}

/// The longest a node name can be. The kubernetes provider names a node's pod,
/// hostname and service after it, and a service name is an RFC 1035 label: at
/// most 63 characters.
pub const NODE_NAME_MAX_LEN: usize = 63;

/// Put in front of a name that doesn't start with a letter, which an RFC 1035
/// label must.
const NODE_NAME_PREFIX: &str = "node-";

/// `name` as a valid RFC 1035 label, the strictest rule any provider applies to
/// a node's name (a kubernetes service name): lowercase letters, digits and
/// `-`, starting with a letter and ending with a letter or digit, at most
/// [`NODE_NAME_MAX_LEN`] characters.
///
/// - Uppercase letters are lowercased (`Collator-1000` -> `collator-1000`).
/// - Any other character becomes `-` (`my_node.1` -> `my-node-1`).
/// - Leading and trailing `-` are dropped, and a name that then doesn't start
///   with a letter gets a `node-` prefix (`1st` -> `node-1st`).
/// - It's cut to [`NODE_NAME_MAX_LEN`].
///
/// An empty name stays empty, so it's still reported as one. A valid name is
/// returned unchanged, so this can be applied any number of times.
pub fn sanitize_node_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }

    let mapped: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let trimmed = mapped.trim_matches('-');
    let mut sanitized = if trimmed.starts_with(|c: char| c.is_ascii_lowercase()) {
        trimmed.to_string()
    } else {
        format!("{NODE_NAME_PREFIX}{trimmed}")
    };

    // Only ASCII is left, so cutting at a byte index is cutting at a char.
    sanitized.truncate(NODE_NAME_MAX_LEN);
    sanitized.trim_end_matches('-').to_string()
}

/// `base` with a `-{counter}` suffix, cut so the whole stays within
/// [`NODE_NAME_MAX_LEN`]. `base` is already sanitized, so it starts with a
/// letter and is ASCII.
fn with_counter(base: &str, counter: usize) -> String {
    let suffix = format!("-{counter}");
    let keep = NODE_NAME_MAX_LEN
        .saturating_sub(suffix.len())
        .min(base.len());
    format!("{}{suffix}", base[..keep].trim_end_matches('-'))
}

/// Returns `node_name`, made a valid node name by [`sanitize_node_name`], if it
/// is not already in `names`.
///
/// Otherwise, appends an incrementing `-{counter}` suffix until a unique name is found,
/// then returns it. Logs a warning when the name had to be adjusted, and when a
/// duplicate is detected.
///
/// Sanitizing comes first, so names that only differ in what is adjusted
/// (`Alice` and `alice`) are still told apart: the second one is `alice-1`.
pub fn generate_unique_node_name_from_names(
    node_name: impl Into<String>,
    names: &mut HashSet<String>,
) -> String {
    let original = node_name.into();
    let node_name = sanitize_node_name(&original);
    if node_name != original {
        warn!(
            original = %original,
            adjusted = %node_name,
            "Node name is not valid on every provider (lowercase letters, digits and '-', \
             starting with a letter, at most {NODE_NAME_MAX_LEN} characters); using the adjusted name."
        );
    }

    if names.insert(node_name.clone()) {
        return node_name;
    }

    let mut counter = 1;
    let mut candidate = node_name.clone();
    while names.contains(&candidate) {
        candidate = with_counter(&node_name, counter);
        counter += 1;
    }

    warn!(
        original = %node_name,
        adjusted = %candidate,
        "Duplicate node name detected."
    );

    names.insert(candidate.clone());
    candidate
}

pub fn ensure_value_is_not_empty(value: &str) -> Result<(), anyhow::Error> {
    if value.is_empty() {
        Err(ValidationError::CantBeEmpty().into())
    } else {
        Ok(())
    }
}

pub fn ensure_port_unique(
    port: Port,
    validation_context: Rc<RefCell<ValidationContext>>,
) -> Result<(), anyhow::Error> {
    let mut context = validation_context
        .try_borrow_mut()
        .expect(&format!("{BORROWABLE}, {THIS_IS_A_BUG}"));

    if !context.used_ports.contains(&port) {
        context.used_ports.push(port);
        return Ok(());
    }

    Err(ValidationError::PortAlreadyUsed(port).into())
}

pub fn generate_unique_para_id(
    para_id: ParaId,
    validation_context: Rc<RefCell<ValidationContext>>,
) -> String {
    let mut context = validation_context
        .try_borrow_mut()
        .expect(&format!("{BORROWABLE}, {THIS_IS_A_BUG}"));

    if let Some(suffix) = context.used_para_ids.get_mut(&para_id) {
        *suffix += 1;
        format!("{para_id}-{suffix}")
    } else {
        // insert 0, since will be used next time.
        context.used_para_ids.insert(para_id, 0);
        para_id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a kubernetes service name must be (RFC 1035 label).
    fn is_rfc1035_label(name: &str) -> bool {
        let bytes = name.as_bytes();
        !name.is_empty()
            && name.len() <= NODE_NAME_MAX_LEN
            && bytes[0].is_ascii_lowercase()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    }

    #[test]
    fn a_valid_name_is_kept_as_is() {
        for name in ["alice", "bob-1", "collator-1000", "a"] {
            assert_eq!(sanitize_node_name(name), name);
        }
    }

    #[test]
    fn an_invalid_name_is_made_a_valid_one() {
        for (name, expected) in [
            ("Alice", "alice"),
            ("Collator-1000", "collator-1000"),
            ("myNode", "mynode"),
            ("my_node.1", "my-node-1"),
            ("my node", "my-node"),
            ("-alice-", "alice"),
            ("1st", "node-1st"),
            ("_", "node"),
            ("Élan", "lan"),
        ] {
            let sanitized = sanitize_node_name(name);
            assert_eq!(sanitized, expected, "{name}");
            assert!(is_rfc1035_label(&sanitized), "{name} -> {sanitized}");
        }
    }

    #[test]
    fn a_long_name_is_cut_to_the_limit() {
        let long = format!("a{}", "-b".repeat(40));
        let sanitized = sanitize_node_name(&long);
        assert!(sanitized.len() <= NODE_NAME_MAX_LEN, "{sanitized}");
        assert!(is_rfc1035_label(&sanitized), "{sanitized}");
    }

    #[test]
    fn sanitizing_twice_changes_nothing() {
        for name in ["Alice", "my_node.1", "1st", "Élan", &"x".repeat(80)] {
            let once = sanitize_node_name(name);
            assert_eq!(sanitize_node_name(&once), once, "{name}");
        }
    }

    #[test]
    fn an_empty_name_stays_empty_to_be_reported() {
        assert_eq!(sanitize_node_name(""), "");
    }

    #[test]
    fn names_that_only_differ_in_case_are_still_unique() {
        let mut names = HashSet::new();
        assert_eq!(
            generate_unique_node_name_from_names("Alice", &mut names),
            "alice"
        );
        assert_eq!(
            generate_unique_node_name_from_names("alice", &mut names),
            "alice-1"
        );
    }

    #[test]
    fn a_counter_suffix_keeps_a_long_name_within_the_limit() {
        let mut names = HashSet::new();
        let long = "n".repeat(NODE_NAME_MAX_LEN);
        assert_eq!(
            generate_unique_node_name_from_names(&long, &mut names),
            long
        );
        let second = generate_unique_node_name_from_names(&long, &mut names);
        assert!(is_rfc1035_label(&second), "{second}");
        assert!(second.ends_with("-1"), "{second}");
    }
}
