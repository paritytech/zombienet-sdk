use nix::sys::signal::Signal;
use rand;
use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::process::ExitStatusExt,
    process::ExitStatus,
    sync::{Arc, RwLock},
};
use tokio::{io::AsyncRead, sync::mpsc};

use async_trait::async_trait;

use super::{Command, DynAsyncRead, DynProcess, Process, ProcessManager};

#[derive(Debug, Clone)]
pub enum FakeProcessState {
    Running,
    Stopped,
}

#[derive(Debug)]
pub struct FakeStdStream(mpsc::Receiver<String>);

impl AsyncRead for FakeStdStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let data = self.0.poll_recv(cx);

        match data {
            std::task::Poll::Ready(Some(chunk)) => {
                buf.put_slice(chunk.as_bytes());
                std::task::Poll::Ready(Ok(()))
            },
            std::task::Poll::Ready(None) => {
                buf.put_slice(&[]);
                std::task::Poll::Ready(Ok(()))
            },
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub struct FakeProcess {
    pub id: u32,
    pub program: OsString,
    pub args: Vec<OsString>,
    pub envs: Vec<(OsString, OsString)>,
    inner: RwLock<FakeProcessInner>,
    process_manager: FakeProcessManager,
}

impl FakeProcess {
    pub async fn state(&self) -> FakeProcessState {
        self.inner.read().unwrap().state.clone()
    }
}

#[derive(Debug)]
struct FakeProcessInner {
    state: FakeProcessState,
    stream_values: Vec<StreamValue>,
    stdout_tx: mpsc::Sender<String>,
    stderr_tx: mpsc::Sender<String>,
    stdout: Option<FakeStdStream>,
    stderr: Option<FakeStdStream>,
}

#[async_trait]
impl Process for FakeProcess {
    async fn id(&self) -> Option<u32> {
        Some(self.id)
    }

    async fn take_stdout(&self) -> Option<DynAsyncRead> {
        self.inner
            .write()
            .unwrap()
            .stdout
            .take()
            .map(|stdout| Box::new(stdout) as DynAsyncRead)
    }

    async fn take_stderr(&self) -> Option<super::DynAsyncRead> {
        self.inner
            .write()
            .unwrap()
            .stderr
            .take()
            .map(|stderr| Box::new(stderr) as DynAsyncRead)
    }

    async fn kill(&self) -> std::io::Result<()> {
        let mut pm_inner = self.process_manager.inner.write().unwrap();

        if let Some(errno) = pm_inner.node_kill_should_error {
            return Err(errno.into());
        } else {
            println!("else");
        }

        pm_inner.processes.remove(&self.id);

        Ok(())
    }
}

#[derive(Clone)]
pub struct DynamicStreamValue(
    Arc<dyn Fn(OsString, Vec<OsString>, Vec<(OsString, OsString)>) -> String + Send + Sync>,
);

impl DynamicStreamValue {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(OsString, Vec<OsString>, Vec<(OsString, OsString)>) -> String + Send + Sync + 'static,
    {
        Self(Arc::new(f))
    }
}

impl std::fmt::Debug for DynamicStreamValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Fn(OsString, Vec<OsString>, Vec<(OsString, OsString)>) -> String"
        )
    }
}

#[derive(Debug, Clone)]
pub enum StreamValue {
    Stdout(String),
    Stderr(String),
    DynamicStdout(DynamicStreamValue),
    DynamicStderr(DynamicStreamValue),
}

#[derive(Clone, Debug)]
pub struct FakeProcessManager {
    inner: Arc<RwLock<FakeProcessManagerInner>>,
}

#[derive(Debug)]
pub struct FakeProcessManagerInner {
    processes: HashMap<u32, Arc<FakeProcess>>,
    streams: HashMap<OsString, Vec<StreamValue>>,
    spawn_should_error: Option<std::io::ErrorKind>,
    output_should_fail: Option<ExitStatus>,
    output_should_error: Option<std::io::ErrorKind>,
    kill_should_error: Option<nix::errno::Errno>,
    node_kill_should_error: Option<nix::errno::Errno>,
}

impl FakeProcessManager {
    pub fn new(streams: HashMap<OsString, Vec<StreamValue>>) -> Self {
        FakeProcessManager {
            inner: Arc::new(RwLock::new(FakeProcessManagerInner {
                processes: HashMap::new(),
                streams,
                spawn_should_error: None,
                output_should_error: None,
                output_should_fail: None,
                kill_should_error: None,
                node_kill_should_error: None,
            })),
        }
    }

    pub fn spawn_should_error(&self, err_kind: std::io::ErrorKind) {
        let mut inner = self.inner.write().unwrap();
        inner.spawn_should_error = Some(err_kind);
    }

    pub fn output_should_error(&self, err_kind: std::io::ErrorKind) {
        let mut inner = self.inner.write().unwrap();
        inner.output_should_error = Some(err_kind);
    }

    pub fn output_should_fail(&self, exit_code: ExitStatus) {
        let mut inner = self.inner.write().unwrap();
        inner.output_should_fail = Some(exit_code);
    }

    pub fn kill_should_error(&self, errno: nix::errno::Errno) {
        let mut inner = self.inner.write().unwrap();
        inner.kill_should_error = Some(errno);
    }

    pub fn node_kill_should_error(&self, errno: nix::errno::Errno) {
        let mut inner = self.inner.write().unwrap();
        inner.node_kill_should_error = Some(errno);
    }

    pub fn push_stream(&self, program: OsString, values: Vec<StreamValue>) {
        self.inner.write().unwrap().streams.insert(program, values);
    }

    pub fn processes(&self) -> Vec<Arc<FakeProcess>> {
        self.inner
            .write()
            .unwrap()
            .processes
            .iter()
            .map(|(_, process)| Arc::clone(process))
            .collect()
    }

    pub fn count(&self) -> usize {
        self.inner.read().unwrap().processes.len()
    }

    pub async fn advance_by(&self, cycles: usize) {
        for (_, process) in self.inner.write().unwrap().processes.iter() {
            let mut inner = process.inner.write().unwrap();

            for _ in 0..cycles {
                if !inner.stream_values.is_empty()
                    && matches!(inner.state, FakeProcessState::Running)
                {
                    let data = inner.stream_values.remove(0);
                    match data {
                        StreamValue::Stdout(stdout_chunk) => {
                            inner.stdout_tx.send(stdout_chunk).await.unwrap()
                        },
                        StreamValue::Stderr(stderr_chunk) => {
                            inner.stderr_tx.send(stderr_chunk).await.unwrap()
                        },
                        StreamValue::DynamicStderr(DynamicStreamValue(f)) => inner
                            .stderr_tx
                            .send(f(
                                process.program.clone(),
                                process.args.clone(),
                                process.envs.clone(),
                            ))
                            .await
                            .unwrap(),
                        StreamValue::DynamicStdout(DynamicStreamValue(f)) => inner
                            .stdout_tx
                            .send(f(
                                process.program.clone(),
                                process.args.clone(),
                                process.envs.clone(),
                            ))
                            .await
                            .unwrap(),
                    };
                }
            }
        }
    }
}

#[async_trait]
impl ProcessManager for FakeProcessManager {
    fn spawn(&self, command: Command) -> std::io::Result<DynProcess> {
        if let Some(err_kind) = self.inner.read().unwrap().spawn_should_error {
            return Err(err_kind.into());
        }

        let mut inner = self.inner.write().unwrap();
        let stream_values = inner
            .streams
            .get(&command.program)
            .cloned()
            .unwrap_or_default();
        let (stdout_tx, stdout_rx) = mpsc::channel(10);
        let (stderr_tx, stderr_rx) = mpsc::channel(10);

        let process = Arc::new(FakeProcess {
            id: rand::random::<u16>() as u32,
            program: command.program,
            args: command.args,
            envs: command.envs,
            inner: RwLock::new(FakeProcessInner {
                state: FakeProcessState::Running,
                stream_values,
                stdout_tx,
                stderr_tx,
                stdout: Some(FakeStdStream(stdout_rx)),
                stderr: Some(FakeStdStream(stderr_rx)),
            }),
            process_manager: self.clone(),
        });

        inner.processes.insert(process.id, Arc::clone(&process));

        Ok(process)
    }

    async fn output(&self, command: Command) -> std::io::Result<std::process::Output> {
        if let Some(err_kind) = self.inner.read().unwrap().output_should_error {
            return Err(err_kind.into());
        }

        let stream_values = self
            .inner
            .read()
            .unwrap()
            .streams
            .get(&command.program)
            .cloned()
            .unwrap_or_default();

        let (stdout, stderr) = stream_values.into_iter().fold(
            (String::new(), String::new()),
            |(mut stdout, mut stderr), value| {
                match value {
                    StreamValue::Stdout(stdout_chunk) => stdout.push_str(&stdout_chunk),
                    StreamValue::Stderr(stderr_chunk) => stderr.push_str(&stderr_chunk),
                    StreamValue::DynamicStdout(DynamicStreamValue(f)) => stdout.push_str(&f(
                        command.program.clone(),
                        command.args.clone(),
                        command.envs.clone(),
                    )),
                    StreamValue::DynamicStderr(DynamicStreamValue(f)) => stderr.push_str(&f(
                        command.program.clone(),
                        command.args.clone(),
                        command.envs.clone(),
                    )),
                }
                (stdout, stderr)
            },
        );

        Ok(std::process::Output {
            status: self
                .inner
                .read()
                .unwrap()
                .output_should_fail
                .unwrap_or(ExitStatus::from_raw(0)),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }

    fn kill<T>(&self, pid: nix::unistd::Pid, signal: T) -> nix::Result<()>
    where
        T: Into<Option<Signal>>,
    {
        if let Some(errno) = self.inner.read().unwrap().kill_should_error {
            return Err(errno);
        }

        let pid = pid.as_raw().try_into().unwrap();
        let processes = &self.inner.write().unwrap().processes;
        let process_state = &mut processes.get(&pid).unwrap().inner.write().unwrap().state;

        match (process_state.clone(), signal.into()) {
            (FakeProcessState::Running, Some(Signal::SIGSTOP)) => {
                *process_state = FakeProcessState::Stopped;
            },
            (FakeProcessState::Stopped, Some(Signal::SIGCONT)) => {
                *process_state = FakeProcessState::Running
            },
            _ => {},
        }

        Ok(())
    }
}
