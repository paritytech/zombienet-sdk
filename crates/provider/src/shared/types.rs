use std::{
    collections::HashMap, fs::File, io::Write, os::unix::process::ExitStatusExt, path::PathBuf,
    process::ExitStatus, vec,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ZombieRole {
    Temp,
    Node,
    BootNode,
    Collator,
    CumulusCollator,
    Authority,
    FullNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PortName {
    Prometheus,
    Rpc,
    RpcWs,
    P2P,
}

#[derive(Debug, Clone, PartialEq)]
enum ImagePullPolicy {
    IfNotPresent,
    Never,
    Always,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMap {
    local_file_path:  PathBuf,
    remote_file_path: PathBuf,
    unique:           bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunCommandResponse {
    pub exit_code: ExitStatus,
    pub std_out:   Vec<u8>,
    pub std_err:   Option<Vec<u8>>,
}

impl RunCommandResponse {
    pub fn default() -> Self {
        Self {
            exit_code: ExitStatus::from_raw(0),
            std_out:   vec![],
            std_err:   None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct NativeRunCommandOptions {
    pub allow_fail: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceLabels {
    job_id:       String,
    project_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceMetadata {
    pub name:   String,
    pub labels: Option<NamespaceLabels>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceDef {
    pub api_version: String,
    pub kind:        String,
    pub metadata:    NamespaceMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodLabels {
    pub zombie_role: ZombieRole,
    pub app:         String,
    pub zombie_ns:   String,
    pub name:        String,
    pub instance:    String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodMetadata {
    pub name:      String,
    pub namespace: String,
    pub labels:    PodLabels,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodSpec {
    pub cfg_path:  String,
    pub data_path: String,
    pub ports:     Vec<Port>,
    pub command:   String,
    pub env:       ProcessEnvironment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodDef {
    pub metadata: PodMetadata,
    pub spec:     PodSpec,
}

type ProcessEnvironment = HashMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Port {
    pub container_port: u16,
    pub name:           PortName,
    pub flag:           String,
    pub host_port:      u16,
}

#[derive(Debug, Clone, PartialEq)]
struct Volume {
    name:       String,
    fs_type:    String,
    mount_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    volumes:                            Option<Vec<Volume>>,
    bootnode:                           Option<bool>,
    bootnode_domain:                    Option<String>,
    timeout:                            u16,
    node_spawn_timeout:                 u16,
    grafana:                            Option<bool>,
    telemetry:                          Option<bool>,
    prometheus:                         Option<bool>,
    /// agent or collator
    jaeger_agent:                       Option<String>,
    /// collator query url
    tracing_collator_url:               Option<String>,
    /// only used by k8s provider and if not set the `url`
    tracing_collator_service_name:      Option<String>,
    /// only used by k8s provider and if not set the `url`
    tracing_collator_service_namespace: Option<String>,
    /// only used by k8s provider and if not set the `url`
    tracing_collator_service_port:      Option<u16>,
    enable_tracing:                     Option<bool>,
    provider:                           String,
    polkadot_introspector:              Option<bool>,
    /// only used in k8s at the moment, spawn a backchannel instance
    backchannel:                        Option<bool>,
    image_pull_policy:                  ImagePullPolicy,
    /// ip used for expose local services (rpc/metrics/monitors)
    local_ip:                           Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Process {
    pub pid:          u32,
    pub log_dir:      String,
    pub port_mapping: HashMap<u16, u16>,
    pub command:      String,
}

#[derive(Debug)]
pub struct LocalFile(File);

impl From<File> for LocalFile {
    fn from(file: File) -> Self {
        LocalFile(file)
    }
}

impl Write for LocalFile {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.0.flush()
    }
}
