use std::error::Error;

use async_trait::async_trait;

use super::types::{NativeRunCommandOptions, PodDef, RunCommandResponse};

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Provider {
    fn create_namespace(&mut self) -> Result<(), Box<dyn Error>>;
    fn setup_cleaner(&self) -> Result<(), Box<dyn Error>>;
    fn get_node_ip(&self) -> Result<String, Box<dyn Error>>;
    fn run_command(
        &self,
        args: Vec<String>,
        opts: NativeRunCommandOptions,
    ) -> Result<RunCommandResponse, Box<dyn Error>>;
    fn create_resource(
        &mut self,
        resourse_def: PodDef,
        scoped: bool,
        wait_ready: bool,
    ) -> Result<(), Box<dyn Error>>;
}
