use std::{collections::HashMap, path::PathBuf};

use anyhow::anyhow;

use crate::types::ExecutionResult;

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct Error(#[from] anyhow::Error);

pub type Result<T> = core::result::Result<T, Error>;

struct DockerContainer {
    command: String,
    created_at: String,
    id: String,
    image: String,
    labels: String,
    local_volumes: String,
    mounts: String,
    names: String,
    networks: String,
    ports: String,
    state: String,
}

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
    rm: bool,
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
        cmd.args(["run", "-d"]);

        if options.rm {
            cmd.arg("--rm");
        }

        if let Some(entrypoint) = options.entrypoint {
            cmd.args(["--entrypoint", &entrypoint]);
        }

        if let Some(volume_mounts) = options.volume_mounts {
            for (source, target) in volume_mounts {
                cmd.args(["-v", &format!("{source}:{target}")]);
            }
        }

        if let Some(name) = options.name {
            cmd.args(["--name", &name]);
        }

        cmd.arg(&options.image);

        for arg in &options.command {
            cmd.arg(arg);
        }

        if let Some(env) = options.env {
            for env_var in env {
                cmd.args(["-e", &format!("{}={}", env_var.0, env_var.1)]);
            }
        }

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

        cmd.arg(name);

        cmd.args(
            command
                .clone()
                .into_iter()
                .map(|s| <S as Into<String>>::into(s)),
        );

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
        local_path: &PathBuf,
        remote_path: &PathBuf,
    ) -> Result<()> {
        let result = tokio::process::Command::new("docker")
            .args([
                "cp",
                local_path.to_string_lossy().as_ref(),
                &format!("{name}:{}", remote_path.to_string_lossy().as_ref()),
            ])
            .output()
            .await
            .map_err(|err| anyhow!("Failed to create volume '{name}': {err}"))?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to copy file '{}' to container '{name}': {}",
                local_path.to_string_lossy(),
                String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }

        Ok(())
    }
}
