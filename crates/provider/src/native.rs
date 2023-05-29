use std::{self, error::Error, fs};

use async_trait::async_trait;

use crate::shared::{
    constants::{DEFAULT_DATA_DIR, DEFAULT_REMOTE_DIR},
    provider::Provider,
    types::{
        FileMap, NamespaceDef, NamespaceMetadata, PodDef, RunCommandOptions, RunCommandResponse,
        Settings,
    },
};

struct NativeProvider {
    /// Namespace of the client
    namespace:             String,
    /// path where configuration relies
    config_path:           String,
    // variable that shows if debug is activated
    debug:                 bool,
    /// the timeout for the client to exit
    timeout:               u32,
    /// command sent to client
    command:               String,
    /// temporary directory
    tmp_dir:               String,
    pod_monitor_available: bool,
    local_magic_file_path: String,
    remote_dir:            String,
    data_dir:              String,
}

impl Default for NativeProvider {
    fn default() -> Self {
        // [TODO]: define the default value for Native
        todo!()
    }
}

impl NativeProvider {
    pub fn new(
        namespace: impl Into<String>,
        config_path: impl Into<String>,
        tmp_dir: impl Into<String>,
    ) -> NativeProvider {
        let tmp_dir = tmp_dir.into();

        NativeProvider {
            namespace: namespace.into(),
            config_path: config_path.into(),
            debug: true,
            timeout: 60, // seconds
            local_magic_file_path: format!("{}/finished.txt", &tmp_dir),
            remote_dir: format!("{}{}", &tmp_dir, DEFAULT_REMOTE_DIR),
            data_dir: format!("{}{}", &tmp_dir, DEFAULT_DATA_DIR),
            command: "bash".into(),
            tmp_dir,
            pod_monitor_available: false,
        }
    }
}

impl Provider for NativeProvider {
    fn create_namespace(&self) -> Result<(), Box<dyn Error>> {
        let name_space_def = NamespaceDef {
            api_version: "v1".into(),
            kind:        "Namespace".into(),
            metadata:    NamespaceMetadata {
                name:   format!("{}", &self.namespace),
                labels: None,
            },
        };

        let file_path = format!("{}/{}", &self.tmp_dir, "namespace");
        std::fs::write(file_path, serde_json::to_string(&name_space_def)?)
            .expect_err("Error during write");

        fs::create_dir(&self.remote_dir)?;
        Ok(())
    }

    // async fn static_setup(settings: Settings) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn destroy_namespace() -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn get_node_logs(
    //     pod_name: String,
    //     since: Option<u32>,
    //     with_timestamp: Option<bool>,
    // ) -> Result<String, Box<dyn Error>> {
    //     todo!()
    // }

    // async fn dump_logs(path: String, pod_name: String) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn upsert_cron_job(minutes: u32) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn start_port_forwarding(
    //     port: u16,
    //     identifier: String,
    //     namespace: Option<String>,
    // ) -> Result<u16, Box<dyn Error>> {
    //     todo!()
    // }

    // async fn run_command(
    //     args: Vec<String>,
    //     opts: RunCommandOptions,
    // ) -> Result<RunCommandResponse, Box<dyn Error>> {
    //     todo!()
    // }

    // async fn run_script(
    //     identifier: String,
    //     script_path: String,
    //     args: Vec<String>,
    // ) -> Result<RunCommandResponse, Box<dyn Error>> {
    //     todo!()
    // }

    // async fn spawn_from_def(
    //     pod_def: PodDef,
    //     files_to_copy: Option<Vec<FileMap>>,
    //     keystore: Option<String>,
    //     chain_spec_id: Option<String>,
    //     db_snapshot: Option<String>,
    // ) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn copy_file_from_pod(
    //     identifier: String,
    //     pod_file_path: String,
    //     local_file_path: String,
    //     container: Option<String>,
    // ) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn put_local_magic_file(
    //     name: String,
    //     container: Option<String>,
    // ) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn create_resource(
    //     resourse_def: NamespaceDef,
    //     scoped: bool,
    //     wait_ready: bool,
    // ) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn create_pod_monitor(file_name: String, chain: String) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn setup_cleaner() -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn is_pod_monitor_available() -> Result<(), bool> {
    //     todo!()
    // }

    // fn get_pause_args(name: String) -> Vec<String> {
    //     todo!()
    // }

    // fn get_resume_args(name: String) -> Vec<String> {
    //     todo!()
    // }

    // async fn restart_node(name: String, timeout: u32) -> Result<(), bool> {
    //     todo!()
    // }

    // async fn get_node_info(
    //     identifier: String,
    //     port: Option<u16>,
    // ) -> Result<Vec<(String, u32)>, Box<dyn Error>> {
    //     todo!()
    // }

    // async fn get_node_ip(identifier: String) -> Result<String, Box<dyn Error>> {
    //     todo!()
    // }

    // async fn spawn_intro_spector(ws_uri: String) -> Result<(), Box<dyn Error>> {
    //     todo!()
    // }

    // async fn validate_access() -> Result<(), bool> {
    //     todo!()
    // }

    // fn get_logs_command(name: String) -> String {
    //     todo!()
    // }
}

#[test]
fn initialize_provider() {
    // namespace: impl Into<String>,
    // config_path: impl Into<String>,
    // tmp_dir: impl Into<String>,
    let mut native_provider = NativeProvider::new("namespace", "./", "./tmp");
    let provider: Box<&mut dyn Provider> = Box::new(&mut native_provider);

    // let future = task::spawn(provider.create_namespace());
}
