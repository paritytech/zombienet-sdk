use std::{collections::HashMap, path::Path};

use anyhow::anyhow;
use serde::{Deserialize, Deserializer};
use tokio::process::Command;
use tracing::trace;

use crate::types::{ExecutionResult, Port};

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct Error(#[from] anyhow::Error);

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone)]
pub struct DockerClient {
    using_podman: bool,
}

#[derive(Debug)]
pub struct ContainerRunOptions {
    image: String,
    command: Vec<String>,
    env: Option<Vec<(String, String)>>,
    volume_mounts: Option<HashMap<String, String>>,
    name: Option<String>,
    entrypoint: Option<String>,
    port_mapping: HashMap<Port, Port>,
    rm: bool,
}

enum Container {
    Docker(DockerContainer),
    Podman(PodmanContainer),
}

// TODO: we may don't need this
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct DockerContainer {
    #[serde(alias = "Names", deserialize_with = "deserialize_list")]
    names: Vec<String>,
    #[serde(alias = "Ports", deserialize_with = "deserialize_list")]
    ports: Vec<String>,
    #[serde(alias = "State")]
    state: String,
}

// TODO: we may don't need this
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct PodmanPort {
    host_ip: String,
    container_port: u16,
    host_port: u16,
    range: u16,
    protocol: String,
}

// TODO: we may don't need this
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct PodmanContainer {
    #[serde(alias = "Id")]
    id: String,
    #[serde(alias = "Image")]
    image: String,
    #[serde(alias = "Mounts")]
    mounts: Vec<String>,
    #[serde(alias = "Names")]
    names: Vec<String>,
    #[serde(alias = "Ports", deserialize_with = "deserialize_null_as_default")]
    ports: Vec<PodmanPort>,
    #[serde(alias = "State")]
    state: String,
}

fn deserialize_list<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let str_sequence = String::deserialize(deserializer)?;
    Ok(str_sequence
        .split(',')
        .filter(|item| !item.is_empty())
        .map(|item| item.to_owned())
        .collect())
}

fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

impl ContainerRunOptions {
    pub fn new<S>(image: &str, command: Vec<S>) -> Self
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        ContainerRunOptions {
            image: image.to_string(),
            command: command
                .clone()
                .into_iter()
                .map(|s| s.into())
                .collect::<Vec<_>>(),
            env: None,
            volume_mounts: None,
            name: None,
            entrypoint: None,
            port_mapping: HashMap::default(),
            rm: false,
        }
    }

    pub fn env<S>(mut self, env: Vec<(S, S)>) -> Self
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        self.env = Some(
            env.into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        );
        self
    }

    pub fn volume_mounts<S>(mut self, volume_mounts: HashMap<S, S>) -> Self
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        self.volume_mounts = Some(
            volume_mounts
                .into_iter()
                .map(|(source, target)| (source.into(), target.into()))
                .collect(),
        );
        self
    }

    pub fn name<S>(mut self, name: S) -> Self
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        self.name = Some(name.into());
        self
    }

    pub fn entrypoint<S>(mut self, entrypoint: S) -> Self
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        self.entrypoint = Some(entrypoint.into());
        self
    }

    pub fn port_mapping(mut self, port_mapping: &HashMap<Port, Port>) -> Self {
        self.port_mapping = port_mapping.clone();
        self
    }

    pub fn rm(mut self) -> Self {
        self.rm = true;
        self
    }
}

impl DockerClient {
    pub async fn new() -> Result<Self> {
        let using_podman = Self::is_using_podman().await?;

        Ok(DockerClient { using_podman })
    }

    async fn is_using_podman() -> Result<bool> {
        let result = tokio::process::Command::new("docker")
            .arg("--version")
            .output()
            .await
            .map_err(|err| anyhow!("Failed to detect container engine: {err}"))?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to detect container engine: {}",
                String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(String::from_utf8_lossy(&result.stdout).contains("podman"))
    }
}

impl DockerClient {
    pub async fn create_volume(&self, name: &str) -> Result<()> {
        let result = tokio::process::Command::new("docker")
            .args(["volume", "create", name])
            .output()
            .await
            .map_err(|err| anyhow!("Failed to create volume '{name}': {err}"))?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to create volume '{name}': {}",
                String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(())
    }

    pub async fn container_run(&self, options: ContainerRunOptions) -> Result<String> {
        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(["run", "-d", "--platform", "linux/amd64"]);

        Self::apply_cmd_options(&mut cmd, &options);

        trace!("cmd: {:?}", cmd);

        let result = cmd.output().await.map_err(|err| {
            anyhow!(
                "Failed to run container with image '{image}' and command '{command}': {err}",
                image = options.image,
                command = options.command.join(" "),
            )
        })?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to run container with image '{image}' and command '{command}': {err}",
                image = options.image,
                command = options.command.join(" "),
                err = String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(String::from_utf8_lossy(&result.stdout).to_string())
    }

    pub async fn container_create(&self, options: ContainerRunOptions) -> Result<String> {
        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(["container", "create"]);

        Self::apply_cmd_options(&mut cmd, &options);

        trace!("cmd: {:?}", cmd);

        let result = cmd.output().await.map_err(|err| {
            anyhow!(
                "Failed to run container with image '{image}' and command '{command}': {err}",
                image = options.image,
                command = options.command.join(" "),
            )
        })?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to run container with image '{image}' and command '{command}': {err}",
                image = options.image,
                command = options.command.join(" "),
                err = String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(String::from_utf8_lossy(&result.stdout).to_string())
    }

    pub async fn container_exec<S>(
        &self,
        name: &str,
        command: Vec<S>,
        env: Option<Vec<(S, S)>>,
        as_user: Option<S>,
    ) -> Result<ExecutionResult>
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        let mut cmd = tokio::process::Command::new("docker");
        cmd.arg("exec");

        if let Some(env) = env {
            for env_var in env {
                cmd.args(["-e", &format!("{}={}", env_var.0.into(), env_var.1.into())]);
            }
        }

        if let Some(user) = as_user {
            cmd.args(["-u", user.into().as_ref()]);
        }

        cmd.arg(name);

        cmd.args(
            command
                .clone()
                .into_iter()
                .map(|s| <S as Into<String>>::into(s)),
        );

        trace!("cmd is : {:?}", cmd);

        let result = cmd.output().await.map_err(|err| {
            anyhow!(
                "Failed to exec '{}' on '{}': {err}",
                command
                    .into_iter()
                    .map(|s| <S as Into<String>>::into(s))
                    .collect::<Vec<_>>()
                    .join(" "),
                name,
            )
        })?;

        if !result.status.success() {
            return Ok(Err((
                result.status,
                String::from_utf8_lossy(&result.stderr).to_string(),
            )));
        }

        Ok(Ok(String::from_utf8_lossy(&result.stdout).to_string()))
    }

    pub async fn container_cp(
        &self,
        name: &str,
        local_path: &Path,
        remote_path: &Path,
    ) -> Result<()> {
        let result = tokio::process::Command::new("docker")
            .args([
                "cp",
                local_path.to_string_lossy().as_ref(),
                &format!("{name}:{}", remote_path.to_string_lossy().as_ref()),
            ])
            .output()
            .await
            .map_err(|err| {
                anyhow!(
                    "Failed copy file '{file}' to container '{name}': {err}",
                    file = local_path.to_string_lossy(),
                )
            })?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to copy file '{file}' to container '{name}': {err}",
                file = local_path.to_string_lossy(),
                err = String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(())
    }

    pub async fn container_rm(&self, name: &str) -> Result<()> {
        let result = tokio::process::Command::new("docker")
            .args(["rm", "--force", "--volumes", name])
            .output()
            .await
            .map_err(|err| anyhow!("Failed do remove container '{name}: {err}"))?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to remove container '{name}': {err}",
                err = String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(())
    }

    pub async fn namespaced_containers_rm(&self, namespace: &str) -> Result<()> {
        let container_names: Vec<String> = self
            .get_containers()
            .await?
            .into_iter()
            .filter_map(|container| match container {
                Container::Docker(container) => {
                    if let Some(name) = container.names.first() {
                        if name.starts_with(namespace) {
                            return Some(name.to_string());
                        }
                    }

                    None
                },
                Container::Podman(container) => {
                    if let Some(name) = container.names.first() {
                        if name.starts_with(namespace) {
                            return Some(name.to_string());
                        }
                    }

                    None
                },
            })
            .collect();

        for name in container_names {
            self.container_rm(&name).await?;
        }

        Ok(())
    }

    pub async fn container_ip(&self, container_name: &str) -> Result<String> {
        let ip = if self.using_podman {
            "127.0.0.1".into()
        } else {
            let mut cmd = tokio::process::Command::new("docker");
            cmd.args(vec![
                "inspect",
                "-f",
                "{{ .NetworkSettings.IPAddress }}",
                container_name,
            ]);

            trace!("CMD: {cmd:?}");

            let res = cmd
                .output()
                .await
                .map_err(|err| anyhow!("Failed to get docker container ip,  output: {err}"))?;

            String::from_utf8(res.stdout)
                .map_err(|err| anyhow!("Failed to get docker container ip,  output: {err}"))?
                .trim()
                .into()
        };

        trace!("IP: {ip}");
        Ok(ip)
    }

    async fn get_containers(&self) -> Result<Vec<Container>> {
        let containers = if self.using_podman {
            self.get_podman_containers()
                .await?
                .into_iter()
                .map(Container::Podman)
                .collect()
        } else {
            self.get_docker_containers()
                .await?
                .into_iter()
                .map(Container::Docker)
                .collect()
        };

        Ok(containers)
    }

    async fn get_podman_containers(&self) -> Result<Vec<PodmanContainer>> {
        let res = tokio::process::Command::new("podman")
            .args(vec!["ps", "--all", "--no-trunc", "--format", "json"])
            .output()
            .await
            .map_err(|err| anyhow!("Failed to get podman containers output: {err}"))?;

        let stdout = String::from_utf8_lossy(&res.stdout);

        let containers = serde_json::from_str(&stdout)
            .map_err(|err| anyhow!("Failed to parse podman containers output: {err}"))?;

        Ok(containers)
    }

    async fn get_docker_containers(&self) -> Result<Vec<DockerContainer>> {
        let res = tokio::process::Command::new("docker")
            .args(vec!["ps", "--all", "--no-trunc", "--format", "json"])
            .output()
            .await
            .unwrap();

        let stdout = String::from_utf8_lossy(&res.stdout);

        let mut containers = vec![];
        for line in stdout.lines() {
            containers.push(
                serde_json::from_str::<DockerContainer>(line)
                    .map_err(|err| anyhow!("Failed to parse docker container output: {err}"))?,
            );
        }

        Ok(containers)
    }

    fn apply_cmd_options(cmd: &mut Command, options: &ContainerRunOptions) {
        if options.rm {
            cmd.arg("--rm");
        }

        if let Some(entrypoint) = options.entrypoint.as_ref() {
            cmd.args(["--entrypoint", entrypoint]);
        }

        if let Some(volume_mounts) = options.volume_mounts.as_ref() {
            for (source, target) in volume_mounts {
                cmd.args(["-v", &format!("{source}:{target}")]);
            }
        }

        if let Some(env) = options.env.as_ref() {
            for env_var in env {
                cmd.args(["-e", &format!("{}={}", env_var.0, env_var.1)]);
            }
        }

        // add published ports
        for (container_port, host_port) in options.port_mapping.iter() {
            cmd.args(["-p", &format!("{host_port}:{container_port}")]);
        }

        if let Some(name) = options.name.as_ref() {
            cmd.args(["--name", name]);
        }

        cmd.arg(&options.image);

        for arg in &options.command {
            cmd.arg(arg);
        }
    }
}
