use std::error::Error;

use async_trait::async_trait;

#[async_trait]
#[allow(non_upper_case_globals)]
pub trait Provider {
    fn create_namespace(&mut self) -> Result<(), Box<dyn Error>>;
    fn setup_cleaner(&self) -> Result<(), Box<dyn Error>>;
    fn get_node_ip(&self) -> Result<String, Box<dyn Error>>;
}
