use std::error::Error;

use async_trait::async_trait;

use super::types::{NativeRunCommandOptions, PodDef, RunCommandResponse};

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Provider {
    fn create_namespace(&mut self) -> Result<(), Box<dyn Error>>;
    fn get_node_ip(&self) -> Result<String, Box<dyn Error>>;
    fn get_port_mapping(&mut self, port: u32, pod_name: String) -> u32;
    async fn get_node_info(&mut self, podName: String) -> Result<(String, u32), Box<dyn Error>>;
    async fn run_command(
        &self,
        args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, Box<dyn Error>>;
    async fn create_resource(&mut self, resourse_def: PodDef) -> Result<(), Box<dyn Error>>;
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
    fn spawn_introspector() -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn static_setup() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn create_static_resource() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn create_pod_monitor() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn setup_cleaner() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn upsert_cron_job() -> Result<(), Box<dyn Error>> {
        todo!();
    }
}
