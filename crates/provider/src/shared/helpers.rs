use std::{path::PathBuf, time::Duration};

use support::fs::FileSystem;
use tokio::{
    io::{AsyncRead, AsyncReadExt, BufReader},
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
    time::sleep,
};

pub(crate) fn create_stream_polling_task<S>(
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

pub(crate) fn create_log_writing_task<FS>(
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
