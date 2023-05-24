#[derive(Debug, Clone, PartialEq)]
pub struct FileMap {
    local_file_path:  String,
    remote_file_path: String,
    unique:           bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunCommandResponse {
    exit_code: u8,
    std_out:   String,
    std_err:   Option<String>,
    error_msg: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunCommandOptions {
    resource_def: Option<String>,
    scoped:       Option<bool>,
    allow_fail:   Option<bool>,
    main_cmd:     String,
}
