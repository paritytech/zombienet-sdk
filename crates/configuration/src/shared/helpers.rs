pub fn merge_errors(errors: Vec<String>, new_error: String) -> Vec<String> {
    vec![errors, vec![new_error]].concat()
}
