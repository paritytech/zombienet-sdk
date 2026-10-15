use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use support::fs::FileSystem;
use tracing::warn;

use crate::{
    types::{RunCommandOptions, TransferedFile},
    DynNode, ProviderError,
};

/// A namespace's base dir: `custom`, created if missing and made absolute,
/// since nodes run from their own directory and docker takes a relative
/// bind-mount source for a named volume; else a fresh `<tmp_dir>/<name>`.
pub(crate) async fn namespace_base_dir<FS: FileSystem>(
    filesystem: &FS,
    tmp_dir: &Path,
    name: &str,
    custom: Option<&Path>,
) -> Result<PathBuf, ProviderError> {
    let Some(custom) = custom else {
        let base_dir = tmp_dir.join(name);
        filesystem.create_dir(&base_dir).await.map_err(|err| {
            ProviderError::CreateNamespaceFailed(
                name.to_string(),
                anyhow!(
                    "can't create base dir {}: {err} (remove it, or set a base_dir)",
                    base_dir.display()
                ),
            )
        })?;
        return Ok(base_dir);
    };
    if filesystem.exists(custom).await {
        warn!(
            "⚠️ Using and existing directory {} as base dir",
            custom.to_string_lossy()
        );
    } else {
        filesystem.create_dir_all(custom).await?;
    }
    std::path::absolute(custom)
        .map_err(|err| ProviderError::CreateNamespaceFailed(name.to_string(), err.into()))
}

/// Check if we are running in `CI` by checking the 'RUN_IN_CI' env var
pub fn running_in_ci() -> bool {
    env::var("RUN_IN_CI").unwrap_or_default() == "1"
}

/// serde default for the fields that older records did not carry.
pub(crate) fn default_true() -> bool {
    true
}

/// The pause/resume/restart controls are messages on the wrapper's pipe. A
/// process started without the wrapper has no pipe, and `echo` into a path
/// that is not a pipe would create a file and report success, so refuse.
pub(crate) fn ensure_wrapper(
    name: &str,
    wrapper: bool,
    action: &str,
    failed: fn(String, anyhow::Error) -> ProviderError,
) -> Result<(), ProviderError> {
    if wrapper {
        Ok(())
    } else {
        Err(failed(
            name.to_string(),
            anyhow!("runs without the zombie wrapper, {action} is not available"),
        ))
    }
}

/// Executes a command on a temporary node and extracts the execution result either from the
/// standard output or a file.
pub async fn extract_execution_result(
    temp_node: &DynNode,
    options: RunCommandOptions,
    expected_path: Option<&PathBuf>,
) -> Result<String, ProviderError> {
    let output_contents = temp_node
        .run_command(options)
        .await?
        .map_err(|(_, msg)| ProviderError::FileGenerationFailed(anyhow!("{msg}")))?;

    // If an expected_path is provided, read the file contents from inside the container
    if let Some(expected_path) = expected_path.as_ref() {
        Ok(temp_node
            .run_command(
                RunCommandOptions::new("cat")
                    .args(vec![expected_path.to_string_lossy().to_string()]),
            )
            .await?
            .map_err(|(_, msg)| {
                ProviderError::FileGenerationFailed(anyhow!(format!(
                    "failed reading expected_path {}: {}",
                    expected_path.display(),
                    msg
                )))
            })?)
    } else {
        Ok(output_contents)
    }
}

pub fn extract_namespace_info(
    json_value: &serde_json::Value,
) -> Result<(PathBuf, String), ProviderError> {
    let base_dir = json_value
        .get("local_base_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or(ProviderError::InvalidConfig(
            "`field local_base_dir` is missing from zombie.json".to_string(),
        ))?;

    let name =
        json_value
            .get("ns")
            .and_then(|v| v.as_str())
            .ok_or(ProviderError::InvalidConfig(
                "field `ns` is missing from zombie.json".to_string(),
            ))?;

    Ok((base_dir, name.to_string()))
}

/// A name for the `index`th injected file of a process: unique by the index,
/// readable by the file name, within what a `ConfigMap` key or a docker
/// mount may contain.
pub(crate) fn injected_file_name(index: usize, file: &TransferedFile) -> String {
    let name: String = file
        .remote_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{index}-{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_runing_in_ci_env_var() {
        assert!(!running_in_ci());
        // now set the env var
        env::set_var("RUN_IN_CI", "1");
        assert!(running_in_ci());
        // reset
        env::set_var("RUN_IN_CI", "");
    }
}
