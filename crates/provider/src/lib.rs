mod errors;
mod native;
mod shared;

use std::{error::Error, path::PathBuf};

use async_trait::async_trait;
use shared::types::{NativeRunCommandOptions, PodDef, RunCommandResponse};
use errors::ProviderError;

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Provider {
    async fn create_namespace(&mut self) -> Result<(), ProviderError>;
    async fn get_node_ip(&self) -> Result<String, ProviderError>;
    async fn get_port_mapping(
        &mut self,
        port: u16,
        pod_name: String,
    ) -> Result<u16, ProviderError>;
    async fn get_node_info(&mut self, pod_name: String) -> Result<(String, u16), ProviderError>;
    async fn run_command(
        &self,
        args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, ProviderError>;
    async fn run_script(
        &mut self,
        identifier: String,
        script_path: String,
        args: Vec<String>,
    ) -> Result<RunCommandResponse, Box<dyn Error>>;
    async fn spawn_from_def(
        &mut self,
        pod_def: PodDef,
        files_to_copy: Vec<FileMap>,
        keystore: String,
        chain_spec_id: String,
        db_snapshot: String,
    ) -> Result<(), Box<dyn ProviderError>>;
    async fn copy_file_from_pod(
        &mut self,
        pod_file_path: PathBuf,
        local_file_path: PathBuf,
    ) -> Result<(), Box<dyn Error>>;
    async fn create_resource(&mut self, resource_def: PodDef, scoped: bool, wait_ready: bool) -> Result<(), ProviderError>;
    async fn wait_node_ready(&mut self, node_name: String) -> Result<(), ProviderError>;
    async fn get_node_logs(&mut self, node_name: String) -> Result<String, ProviderError>;
    async fn dump_logs(&mut self, path: String, pod_name: String) -> Result<(), Box<dyn Error>>;
    fn get_pause_args(&mut self, name: String) -> Vec<String>;
    fn get_resume_args(&mut self, name: String) -> Vec<String>;
    async fn validate_access(&mut self) -> Result<bool, Box<dyn Error>>;
    async fn destroy_namespace(&mut self) -> Result<(), Box<dyn Error>>;
    fn is_pod_monitor_available() -> Result<bool, Box<dyn Error>> {
        Ok(false)
    }
    async fn spawn_introspector() -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    async fn static_setup() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    async fn create_static_resource() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    async fn create_pod_monitor() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    async fn setup_cleaner() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    async fn upsert_cron_job() -> Result<(), Box<dyn Error>> {
        todo!();
    }
}
