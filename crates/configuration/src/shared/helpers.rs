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

pub fn generate_unique_node_name(
    node_name: impl Into<String>,
    validation_context: Rc<RefCell<ValidationContext>>,
) -> String {
    let mut context = validation_context
        .try_borrow_mut()
        .expect(&format!("{BORROWABLE}, {THIS_IS_A_BUG}"));

    generate_unique_node_name_from_names(node_name, &mut context.used_nodes_names)
}

pub fn generate_unique_node_name_from_names(
    node_name: impl Into<String>,
    names: &mut HashSet<String>,
) -> String {
    let node_name = node_name.into();

    if names.insert(node_name.clone()) {
        return node_name;
    }

    let mut counter = 1;
    let mut candidate = node_name.clone();
    while names.contains(&candidate) {
        candidate = format!("{node_name}-{counter}");
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
