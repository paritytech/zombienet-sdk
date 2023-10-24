use std::{path::PathBuf, process::Stdio, time::Duration};

use support::{
    fs::FileSystem,
    process::{Command, DynProcess, ProcessManager},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, BufReader},
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
    time::sleep,
};

use crate::ProviderError;

pub fn create_stream_polling_task<S>(
    stream: S,
    tx: Sender<Result<Vec<u8>, std::io::Error>>,
) -> JoinHandle<()>
where
    S: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut buffer = vec![0u8; 1024];

        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => {
                    let _ = tx.send(Ok(Vec::new())).await;
                    break;
                },
                Ok(n) => {
                    let _ = tx.send(Ok(buffer[..n].to_vec())).await;
                },
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                },
            }
        }
    })
}

pub fn create_log_writing_task<FS>(
    mut rx: Receiver<Result<Vec<u8>, tokio::io::Error>>,
    filesystem: FS,
    log_path: PathBuf,
) -> JoinHandle<()>
where
    FS: FileSystem + Send + Sync + 'static,
{
    tokio::spawn(async move {
        loop {
            while let Some(Ok(data)) = rx.recv().await {
                // TODO: find a better way instead of ignoring error ?
                let _ = filesystem.append(&log_path, data).await;
            }
            sleep(Duration::from_millis(250)).await;
        }
    })
}

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
