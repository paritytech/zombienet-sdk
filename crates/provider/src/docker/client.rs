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

    pub async fn container_run<S>(
        &self,
        image: &str,
        rm: bool,
        command: Vec<S>,
        env: Option<Vec<(S, S)>>,
        volume_mounts: Option<HashMap<S, S>>,
        name: Option<&str>,
        entrypoint: Option<&str>,
    ) -> Result<String>
    where
        S: Into<String> + std::fmt::Debug + Send + Clone,
    {
        let mut cmd = tokio::process::Command::new("docker");
        cmd.args(["run"]);

        if rm {
            cmd.arg("--rm");
        }

        if let Some(volume_mounts) = volume_mounts {
            for (source, target) in volume_mounts {
                cmd.args(["-v", &format!("{}:{}", source.into(), target.into())]);
            }
        }

        if let Some(name) = name {
            cmd.args(["--name", name]);
        }

        cmd.arg(image);

        for arg in command.clone() {
            cmd.arg(arg.into());
        }

        if let Some(env) = env {
            for env_var in env {
                cmd.args(["-e", &format!("{}={}", env_var.0.into(), env_var.1.into())]);
            }
        }

        let result = cmd.output().await.map_err(|err| {
            anyhow!(
                "Failed to run container with image '{image}' and command '{}': {err}",
                command
                    .clone()
                    .into_iter()
                    .map(|s| s.into())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        })?;

        if !result.status.success() {
            return Err(anyhow!(
                "Failed to run container with image '{image}' and command '{}': {}",
                command
                    .into_iter()
                    .map(|s| s.into())
                    .collect::<Vec<_>>()
                    .join(" "),
                String::from_utf8_lossy(&result.stderr)
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
