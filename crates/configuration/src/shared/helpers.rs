use std::{cell::RefCell, rc::Rc};

use support::constants::{BORROWABLE, THIS_IS_A_BUG};

use super::{
    errors::ValidationError,
    types::{Port, ValidationContext},
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

pub fn ensure_node_name_unique(
    node_name: impl Into<String>,
    validation_context: Rc<RefCell<ValidationContext>>,
) -> Result<(), anyhow::Error> {
    let mut context = validation_context
        .try_borrow_mut()
        .expect(&format!("{}, {}", BORROWABLE, THIS_IS_A_BUG));

    let node_name = node_name.into();
    if !context.used_nodes_names.contains(&node_name) {
        context.used_nodes_names.push(node_name);
        return Ok(());
    }

    Err(ValidationError::NodeNameAlreadyUsed(node_name).into())
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
        .expect(&format!("{}, {}", BORROWABLE, THIS_IS_A_BUG));

    if !context.used_ports.contains(&port) {
        context.used_ports.push(port);
        return Ok(());
    }

    Err(ValidationError::PortAlreadyUsed(port).into())
}
