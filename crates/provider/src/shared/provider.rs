use std::error::Error;

use async_trait::async_trait;
use napi_derive::napi;

use super::types::{FileMap, NamespaceDef, PodDef, Settings};
use crate::shared::types::{RunCommandOptions, RunCommandResponse};

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Provider {
    fn create_namespace(&self) -> Result<(), Box<dyn Error>>;
    fn setup_cleaner(&self) -> Result<(), Box<dyn Error>>;
    fn get_node_ip(&self) -> Result<String, Box<dyn Error>>;
    // async fn run_command(
    //     args: Vec<String>,
    //     opts: RunCommandOptions,
    // ) -> Result<RunCommandResponse, Box<dyn Error>>;
    // async fn static_setup(settings: Settings) -> Result<(), Box<dyn Error>>;
    // async fn destroy_namespace() -> Result<(), Box<dyn Error>>;
    // async fn get_node_logs(
    //     pod_name: String,
    //     since: Option<u32>,
    //     with_timestamp: Option<bool>,
    // ) -> Result<String, Box<dyn Error>>;
    // async fn dump_logs(path: String, pod_name: String) -> Result<(), Box<dyn Error>>;
    // async fn upsert_cron_job(minutes: u32) -> Result<(), Box<dyn Error>>;
    // async fn start_port_forwarding(
    //     port: u16,
    //     identifier: String,
    //     namespace: Option<String>,
    // ) -> Result<u16, Box<dyn Error>>;
    // async fn run_script(
    //     identifier: String,
    //     script_path: String,
    //     args: Vec<String>,
    // ) -> Result<RunCommandResponse, Box<dyn Error>>;
    // async fn spawn_from_def(
    //     pod_def: PodDef,
    //     files_to_copy: Option<Vec<FileMap>>,
    //     keystore: Option<String>,
    //     chain_spec_id: Option<String>,
    //     db_snapshot: Option<String>,
    // ) -> Result<(), Box<dyn Error>>;
    // async fn copy_file_from_pod(
    //     identifier: String,
    //     pod_file_path: String,
    //     local_file_path: String,
    //     container: Option<String>,
    // ) -> Result<(), Box<dyn Error>>;
    // async fn put_local_magic_file(
    //     name: String,
    //     container: Option<String>,
    // ) -> Result<(), Box<dyn Error>>;
    // async fn create_resource(
    //     resourse_def: NamespaceDef,
    //     scoped: bool,
    //     wait_ready: bool,
    // ) -> Result<(), Box<dyn Error>>;
    // async fn create_pod_monitor(file_name: String, chain: String) -> Result<(), Box<dyn Error>>;
    // async fn is_pod_monitor_available() -> Result<(), bool>;
    // fn get_pause_args(name: String) -> Vec<String>;
    // fn get_resume_args(name: String) -> Vec<String>;
    // async fn restart_node(name: String, timeout: u32) -> Result<(), bool>;
    // async fn get_node_info(
    //     identifier: String,
    //     port: Option<u16>,
    // ) -> Result<Vec<(String, u32)>, Box<dyn Error>>;
    // async fn spawn_intro_spector(ws_uri: String) -> Result<(), Box<dyn Error>>;
    // async fn validate_access() -> Result<(), bool>;
    // fn get_logs_command(name: String) -> String;
}
