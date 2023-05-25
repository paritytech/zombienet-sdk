use std::error::Error;

use crate::shared::{
    client::Client,
    types::{FileMap, NameSpaceDef, PodDef, RunCommandOptions, RunCommandResponse, Settings},
};

struct Native {
    // Namespace of the client
    namespace:             String,
    // path where configuration relies
    config_path:           String,
    // variable that shows if debug is activated
    debug:                 bool,
    // the timeout for the client to exit
    timeout:               u32,
    // command sent to client
    command:               String,
    // temporary directory
    tmp_dir:               String,
    pod_monitor_available: bool,
    local_magic_file_path: String,
    // name of the provider
    provider_name:         String,
}

impl Default for Native {
    fn default() -> Self {
        // [TODO]: define the default value for Native
        todo!()
    }
}

impl Native {
    pub fn new(namespace: &str, config_path: &str, tmp_dir: &str) -> Native {
        Native {
            namespace:             namespace.to_owned(),
            config_path:           config_path.to_owned(),
            debug:                 true,
            timeout:               60, // seconds
            tmp_dir:               tmp_dir.to_owned(),
            command:               "bash".to_owned(),
            pod_monitor_available: false,
            local_magic_file_path: format!("{}/finished.txt", tmp_dir),
            provider_name:         "native".to_owned(),
        }
    }
}

impl Client for Native {
    fn create_namespace<'async_trait>() -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn static_setup<'async_trait>(
        settings: Settings,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn destroy_namespace<'async_trait>() -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn get_node_logs<'async_trait>(
        pod_name: String,
        since: Option<u32>,
        with_timestamp: Option<bool>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<String, Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn dump_logs<'async_trait>(
        path: String,
        pod_name: String,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn upsert_cron_job<'async_trait>(
        minutes: u32,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn start_port_forwarding<'async_trait>(
        port: u16,
        identifier: String,
        namespace: Option<String>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<u16, Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn run_command<'async_trait>(
        args: Vec<String>,
        opts: RunCommandOptions,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<RunCommandResponse, Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn run_script<'async_trait>(
        identifier: String,
        script_path: String,
        args: Vec<String>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<RunCommandResponse, Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn spawn_from_def<'async_trait>(
        pod_def: PodDef,
        files_to_copy: Option<Vec<FileMap>>,
        keystore: Option<String>,
        chain_spec_id: Option<String>,
        db_snapshot: Option<String>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn copy_file_from_pod<'async_trait>(
        identifier: String,
        pod_file_path: String,
        local_file_path: String,
        container: Option<String>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn put_local_magic_file<'async_trait>(
        name: String,
        container: Option<String>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn create_resource<'async_trait>(
        resourse_def: NameSpaceDef,
        scoped: bool,
        wait_ready: bool,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn create_pod_monitor<'async_trait>(
        file_name: String,
        chain: String,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn setup_cleaner<'async_trait>() -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn is_pod_monitor_available<'async_trait>() -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), bool>> + core::marker::Send + 'async_trait,
        >,
    > {
        todo!()
    }

    fn get_pause_args(name: String) -> Vec<String> {
        todo!()
    }

    fn get_resume_args(name: String) -> Vec<String> {
        todo!()
    }

    fn restart_node<'async_trait>(
        name: String,
        timeout: u32,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), bool>> + core::marker::Send + 'async_trait,
        >,
    > {
        todo!()
    }

    fn get_node_info<'async_trait>(
        identifier: String,
        port: Option<u16>,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<Vec<(String, u32)>, Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn get_node_ip<'async_trait>(
        identifier: String,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<String, Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn spawn_intro_spector<'async_trait>(
        ws_uri: String,
    ) -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), Box<dyn Error>>>
                + core::marker::Send
                + 'async_trait,
        >,
    > {
        todo!()
    }

    fn validate_access<'async_trait>() -> core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Result<(), bool>> + core::marker::Send + 'async_trait,
        >,
    > {
        todo!()
    }

    fn get_logs_command(name: String) -> String {
        todo!()
    }
}

fn main() {
    let mut some: Native = Native::new("namespace", "config_path", "tmp_dir");
}
