mod native;
mod shared;
mod errors;

use std::error::Error;
use async_trait::async_trait;

use shared::types::{NativeRunCommandOptions, PodDef, RunCommandResponse};

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Provider {
    async fn create_namespace(&mut self) -> Result<(), Box<dyn Error>>;
    async fn get_node_ip(&self) -> Result<String, Box<dyn Error>>;
    async fn get_port_mapping(&mut self, port: u16, pod_name: String) -> Result<u16, Box<dyn Error>>;
    async fn get_node_info(&mut self, pod_name: String) -> Result<(String, u16), Box<dyn Error>>;
    async fn run_command(
        &self,
        args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, Box<dyn Error>>;
    async fn create_resource(&mut self, resource_def: PodDef) -> Result<(), Box<dyn Error>>;
    async fn wait_node_ready(&mut self, node_name: String) -> Result<(), Box<dyn Error>>;
    async fn get_node_logs(&mut self, node_name: String) -> Result<String, Box<dyn Error>>;
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
