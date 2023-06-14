use std::error::Error;

pub fn merge_errors(errors: Vec<Box<dyn Error>>, new_error: Box<dyn Error>) -> Vec<Box<dyn Error>> {
    let mut errors = errors;
    errors.push(new_error);

    errors
}

pub fn merge_errors_vecs(
    errors: Vec<Box<dyn Error>>,
    new_errors: Vec<Box<dyn Error>>,
) -> Vec<Box<dyn Error>> {
    let mut errors = errors;

    for new_error in new_errors.into_iter() {
        errors.push(new_error);
    }

    errors
}
