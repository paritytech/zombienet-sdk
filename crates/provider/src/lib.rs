mod errors;
mod native;
mod shared;

use std::{net::IpAddr, path::PathBuf};

use async_trait::async_trait;
use errors::ProviderError;
use shared::types::{FileMap, NativeRunCommandOptions, PodDef, Port, RunCommandResponse};

#[async_trait]
pub trait Provider {
    async fn create_namespace(&mut self) -> Result<(), ProviderError>;
    async fn get_node_ip(&self) -> Result<IpAddr, ProviderError>;
    async fn get_port_mapping(
        &mut self,
        port: Port,
        pod_name: String,
    ) -> Result<Port, ProviderError>;
    async fn get_node_info(&mut self, pod_name: String) -> Result<(IpAddr, Port), ProviderError>;
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
    ) -> Result<RunCommandResponse, ProviderError>;
    async fn spawn_from_def(
        &mut self,
        pod_def: PodDef,
        files_to_copy: Vec<FileMap>,
        keystore: String,
        chain_spec_id: String,
        db_snapshot: String,
    ) -> Result<(), ProviderError>;
    async fn copy_file_from_pod(
        &mut self,
        pod_file_path: PathBuf,
        local_file_path: PathBuf,
    ) -> Result<(), ProviderError>;
    async fn create_resource(
        &mut self,
        resource_def: PodDef,
        scoped: bool,
        wait_ready: bool,
    ) -> Result<(), ProviderError>;
    async fn wait_node_ready(&mut self, node_name: String) -> Result<(), ProviderError>;
    async fn get_node_logs(&mut self, node_name: String) -> Result<String, ProviderError>;
    async fn dump_logs(&mut self, path: String, pod_name: String) -> Result<(), ProviderError>;
    fn get_pause_args(&mut self, name: String) -> Vec<String>;
    fn get_resume_args(&mut self, name: String) -> Vec<String>;
    async fn restart_node(&mut self, name: String, timeout: u64) -> Result<bool, ProviderError>;
    async fn get_help_info(&mut self) -> Result<bool, ProviderError>;
    async fn destroy_namespace(&mut self) -> Result<(), ProviderError>;
    async fn get_logs_command(&mut self, name: String) -> Result<String, ProviderError>;
    // TODO: need to implement
    async fn put_local_magic_file(
        &self,
        _name: String,
        _container: Option<String>,
    ) -> Result<(), ProviderError> {
        Ok(())
    }
    fn is_pod_monitor_available() -> Result<bool, ProviderError> {
        Ok(false)
    }
    async fn spawn_introspector() -> Result<(), ProviderError> {
        Ok(())
    }

    async fn static_setup() -> Result<(), ProviderError> {
        Ok(())
    }
    async fn create_static_resource() -> Result<(), ProviderError> {
        Ok(())
    }
    async fn create_pod_monitor() -> Result<(), ProviderError> {
        Ok(())
    }
    async fn setup_cleaner() -> Result<(), ProviderError> {
        Ok(())
    }
    async fn upsert_cron_job() -> Result<(), ProviderError> {
        unimplemented!();
    }
}

// re-exports
pub use native::NativeProvider;
