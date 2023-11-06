use std::{path::PathBuf, process::Stdio};

use support::{
    fs::FileSystem,
    process::{Command, DynProcess, ProcessManager},
};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{
    shared::helpers::{create_log_writing_task, create_stream_polling_task},
    ProviderError,
};

type CreateProcessOutput = (DynProcess, JoinHandle<()>, JoinHandle<()>, JoinHandle<()>);

pub(super) async fn create_process_with_log_tasks<FS, PM>(
    node_name: &str,
    program: &str,
    args: &Vec<String>,
    env: &Vec<(String, String)>,
    log_path: &PathBuf,
    filesystem: FS,
    process_manager: PM,
) -> Result<CreateProcessOutput, ProviderError>
where
    FS: FileSystem + Send + Sync + 'static,
    PM: ProcessManager + Send + Sync + 'static,
{
    // create process
    let process = process_manager
        .spawn(
            Command::new(program)
                .args(args)
                .envs(env.to_owned())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true),
        )
        .await
        .map_err(|err| ProviderError::NodeSpawningFailed(node_name.to_string(), err.into()))?;
    let stdout = process
        .take_stdout()
        .await
        .expect("infaillible, stdout is piped");
    let stderr = process
        .take_stderr()
        .await
        .expect("Infaillible, stderr is piped");

    // create additionnal long-running tasks for logs
    let (stdout_tx, rx) = mpsc::channel(10);
    let stderr_tx = stdout_tx.clone();
    let stdout_reading_handle = create_stream_polling_task(stdout, stdout_tx);
    let stderr_reading_handle = create_stream_polling_task(stderr, stderr_tx);
    let log_writing_handle = create_log_writing_task(rx, filesystem, log_path.to_owned());

    Ok((
        process,
        stdout_reading_handle,
        stderr_reading_handle,
        log_writing_handle,
    ))
}
