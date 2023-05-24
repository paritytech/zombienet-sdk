use std::any::Any;

use async_trait::async_trait;

use super::types::FileMap;
use crate::shared::types::{RunCommandOptions, RunCommandResponse};

#[allow(dead_code)]
pub enum CustomError {
    IoError(std::io::Error),
    ParseError(std::num::ParseIntError),
}

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Client {
    // Namespace of the client
    const namespace: String;
    // path where configuration relies
    const config_path: String;
    // variable that shows if debug is activated
    const debug: bool;
    // the timeout for the client to exit
    const timeout: u32;
    // command sent to client
    const command: String;
    // temporary directory
    const tmp_dir: String;
    const pod_monitor_available: bool;
    const local_magic_file_path: String;
    // name of the provider
    const provider_name: String;
    const remote_dir: String;
    async fn create_namespace() -> Result<(), CustomError>;
    async fn static_setup(settings: dyn Any) -> Result<(), CustomError>;
    async fn destroy_namespace() -> Result<(), CustomError>;
    async fn get_node_logs(
        pod_name: String,
        since: Option<u32>,
        with_timestamp: Option<bool>,
    ) -> Result<String, CustomError>;
    async fn dump_logs(path: String, pod_name: String) -> Result<(), CustomError>;
    async fn upsert_cron_job(minutes: u32) -> Result<(), CustomError>;
    async fn start_port_forwarding(
        port: u16,
        identifier: String,
        namespace: Option<String>,
    ) -> Result<u16, CustomError>;
    async fn run_command(
        args: Vec<String>,
        opts: RunCommandOptions,
    ) -> Result<RunCommandResponse, CustomError>;
    async fn run_script(
        identifier: String,
        script_path: String,
        args: Vec<String>,
    ) -> Result<RunCommandResponse, CustomError>;
    async fn spawn_from_def(
        pod_def: dyn Any,
        files_to_copy: Option<Vec<FileMap>>,
        keystore: Option<String>,
        chain_spec_id: Option<String>,
        db_snapshot: Option<String>,
    ) -> Result<(), CustomError>;
    async fn copy_file_from_pod(
        identifier: String,
        pod_file_path: String,
        local_file_path: String,
        container: Option<String>,
    ) -> Result<(), CustomError>;
    async fn put_local_magic_file(
        name: String,
        container: Option<String>,
    ) -> Result<(), CustomError>;
    async fn create_resource(
        resourse_def: dyn Any,
        scoped: bool,
        wait_ready: bool,
    ) -> Result<(), CustomError>;
    async fn create_pod_monitor(file_name: String, chain: String) -> Result<(), CustomError>;
    async fn setup_cleaner() -> Result<(), CustomError>;
    async fn is_pod_monitor_available() -> Result<(), bool>;
    fn get_pause_args(name: String) -> Vec<String>;
    fn get_resume_args(name: String) -> Vec<String>;
    async fn restart_node(name: String, timeout: u32) -> Result<(), bool>;
    async fn get_node_info(
        identifier: String,
        port: Option<u16>,
    ) -> Result<Vec<(String, u32)>, CustomError>;
    async fn get_node_ip(identifier: String) -> Result<String, CustomError>;
    async fn spawn_intro_spector(ws_uri: String) -> Result<(), CustomError>;
    async fn validate_access() -> Result<(), bool>;
    fn get_logs_command(name: String) -> String;
}
