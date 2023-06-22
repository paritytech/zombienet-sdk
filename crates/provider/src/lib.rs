mod errors;
mod native;
mod shared;

use std::{net::IpAddr, path::PathBuf};

use async_trait::async_trait;
use errors::ProviderError;
use shared::types::{FileMap, NativeRunCommandOptions, Node, Port, RunCommandResponse};

#[async_trait]
pub trait Provider {
    /// Create namespace
    async fn create_namespace(&mut self) -> Result<(), ProviderError>;
    /// Destroy namespace (and inner resources).
    async fn destroy_namespace(&self) -> Result<(), ProviderError>;
    /// Spawn a long live node/process.
    async fn spawn_node(
        &mut self,
        node: Node,
        // Files to inject, `before` we run the provider command.
        files_inject: Vec<FileMap>,
        // TODO: keystore logic should live in the orchestrator
        keystore: &str,
        // chain_spec_id: String,
        // TODO: abstract logic for download and uncompress
        db_snapshot: &str,
    ) -> Result<(), ProviderError>;
    /// Spawn a temporary node, will be shutodown after `get` the desired files or output.
    async fn spawn_temp(
        &self,
        node: Node,
        // Files to inject, `before` we run the provider command.
        files_inject: Vec<FileMap>,
        // Files to get, `after` we run the provider command.
        files_get: Vec<FileMap>,
    ) -> Result<(), ProviderError>;
    /// Copy a single file from node to local filesystem.
    async fn copy_file_from_node(
        &mut self,
        node_file_path: PathBuf,
        local_file_path: PathBuf,
    ) -> Result<(), ProviderError>;
    /// Run a command inside the node.
    async fn run_command(
        &self,
        args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, ProviderError>;
    /// Run a script inside the node, should be a shell script and zombienet will
    /// upload the content first.
    async fn run_script(
        &mut self,
        identifier: String,
        script_path: String,
        args: Vec<String>,
    ) -> Result<RunCommandResponse, ProviderError>;
    async fn get_node_logs(&mut self, node_name: &str) -> Result<String, ProviderError>;
    async fn dump_logs(&mut self, path: String, node_name: String) -> Result<(), ProviderError>;
    async fn get_logs_command(&self, node_name: &str) -> Result<String, ProviderError>;
    async fn pause(&self, node_name: &str) -> Result<(), ProviderError>;
    async fn resume(&self, node_name: &str) -> Result<(), ProviderError>;
    async fn restart(
        &mut self,
        node_name: &str,
        after_sec: Option<u16>,
    ) -> Result<bool, ProviderError>;
    async fn get_node_info(&self, node_name: &str) -> Result<(IpAddr, Port), ProviderError>;
    async fn get_node_ip(&self, node_name: &str) -> Result<IpAddr, ProviderError>;
    async fn get_port_mapping(&self, port: Port, node_name: &str) -> Result<Port, ProviderError>;
    async fn static_setup() -> Result<(), ProviderError> {
        unimplemented!()
    }
    async fn create_static_resource() -> Result<(), ProviderError> {
        unimplemented!()
    }
}

// re-exports
pub use native::NativeProvider;
