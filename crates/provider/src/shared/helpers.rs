use std::{env, path::PathBuf};

use anyhow::anyhow;

use crate::{types::RunCommandOptions, DynNode, ProviderError};

/// Check if we are running in `CI` by checking the 'RUN_IN_CI' env var
pub fn running_in_ci() -> bool {
    env::var("RUN_IN_CI").unwrap_or_default() == "1"
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
