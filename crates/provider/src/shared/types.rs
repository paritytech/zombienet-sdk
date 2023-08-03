use std::{
    collections::HashMap, os::unix::process::ExitStatusExt, path::PathBuf, process::ExitStatus,
};
use serde::{Deserialize, Serialize};

use configuration::types::{Port, Command, Arg, EnvVar, Image};

//  `Node` spec from the provider point of view, this are the fields
//  that any `Provider` implementation must use to spawn nodes
pub struct Node {
    command: Command,
    args: Vec<Arg>,
    env: Vec<EnvVar>,
    ports: HashMap<String, Port>,
    image: Option<Image>
    // resources
    // imagePullPolicy
    // dbSnapshot
}

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
    Other
}

// TODO: remove when we implement k8s/podman
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum ImagePullPolicy {
    IfNotPresent,
    Never,
    Always,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMap {
    pub local_file_path: PathBuf,
    pub remote_file_path: PathBuf,
    pub is_unique: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunCommandResponse {
    pub exit_code: ExitStatus,
    pub std_out: String,
    pub std_err: Option<String>,
}

impl RunCommandResponse {
    pub fn default() -> Self {
        Self {
            exit_code: ExitStatus::from_raw(0),
            std_out: String::default(),
            std_err: None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct NativeRunCommandOptions {
    pub is_failure_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceLabels {
    job_id: String,
    project_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceMetadata {
    pub name: String,
    pub labels: Option<NamespaceLabels>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceDef {
    pub api_version: String,
    pub kind: String,
    pub metadata: NamespaceMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodLabels {
    pub zombie_role: ZombieRole,
    pub app: String,
    pub zombie_ns: String,
    pub name: String,
    pub instance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodMetadata {
    pub name: String,
    pub namespace: String,
    pub labels: PodLabels,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PodSpec {
    pub cfg_path: String,
    pub data_path: String,
    pub ports: Vec<PortInfo>,
    pub command: Vec<String>,
    pub env: ProcessEnvironment,
}

#[derive(Debug, Clone, PartialEq)]
struct PodDef {
    pub metadata: PodMetadata,
    pub spec: PodSpec,
}


type ProcessEnvironment = Vec<EnvVar>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortInfo {
    pub container_port: Port,
    pub name: PortName,
    pub flag: String,
    pub host_port: Port,
}

#[derive(Debug, Clone, PartialEq)]
struct Volume {
    name: String,
    fs_type: String,
    mount_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    volumes: Option<Vec<Volume>>,
    bootnode: Option<bool>,
    bootnode_domain: Option<String>,
    timeout: u16,
    node_spawn_timeout: u16,
    grafana: Option<bool>,
    telemetry: Option<bool>,
    prometheus: Option<bool>,
    /// agent or collator
    jaeger_agent: Option<String>,
    /// collator query url
    tracing_collator_url: Option<String>,
    /// only used by k8s provider and if not set the `url`
    tracing_collator_service_name: Option<String>,
    /// only used by k8s provider and if not set the `url`
    tracing_collator_service_namespace: Option<String>,
    /// only used by k8s provider and if not set the `url`
    tracing_collator_service_port: Option<u16>,
    enable_tracing: Option<bool>,
    provider: String,
    polkadot_introspector: Option<bool>,
    /// only used in k8s at the moment, spawn a backchannel instance
    backchannel: Option<bool>,
    image_pull_policy: ImagePullPolicy,
    /// ip used for expose local services (rpc/metrics/monitors)
    local_ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub logs: String,
    pub port_mapping: HashMap<u16, u16>,
    pub command: String,
    pub env: ProcessEnvironment,
}
