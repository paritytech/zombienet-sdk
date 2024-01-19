use std::{
    ffi::{OsStr, OsString},
    fmt::Debug,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use async_trait::async_trait;
use tokio::io::AsyncRead;

pub mod fake;
pub mod os;

#[derive(Debug)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
    kill_on_drop: bool,
    current_dir: Option<PathBuf>,
}

impl Command {
    pub fn new<S>(program: S) -> Self
    where
        S: AsRef<OsStr>,
    {
        Self {
            program: program.as_ref().to_os_string(),
            args: vec![],
            envs: vec![],
            stdin: None,
            stdout: None,
            stderr: None,
            kill_on_drop: false,
            current_dir: None,
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect();
        self
    }

    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.envs = vars
            .into_iter()
            .map(|(key, val)| (key.as_ref().to_os_string(), val.as_ref().to_os_string()))
            .collect();
        self
    }

    pub fn stdin<T>(mut self, cfg: T) -> Self
    where
        T: Into<Stdio>,
    {
        self.stdin = Some(cfg.into());
        self
    }

    pub fn stdout<T>(mut self, cfg: T) -> Self
    where
        T: Into<Stdio>,
    {
        self.stdout = Some(cfg.into());
        self
    }

    pub fn stderr<T>(mut self, cfg: T) -> Self
    where
        T: Into<Stdio>,
    {
        self.stderr = Some(cfg.into());
        self
    }

    pub fn kill_on_drop(mut self, kill_on_drop: bool) -> Self {
        self.kill_on_drop = kill_on_drop;
        self
    }

    pub fn current_dir(mut self, current_dir: impl AsRef<Path>) -> Self {
        self.current_dir = Some(current_dir.as_ref().into());
        self
    }
}

pub type DynAsyncRead = Box<dyn AsyncRead + Send + Unpin>;

#[async_trait]
pub trait Process: Debug {
    async fn id(&self) -> Option<u32>;
    async fn take_stdout(&self) -> Option<DynAsyncRead>;
    async fn take_stderr(&self) -> Option<DynAsyncRead>;
    async fn kill(&self) -> io::Result<()>;
}

pub type DynProcess = Arc<dyn Process + Send + Sync>;

#[async_trait]
pub trait ProcessManager {
    async fn spawn(&self, command: Command) -> io::Result<DynProcess>;

    async fn output(&self, command: Command) -> io::Result<std::process::Output>;

    async fn kill<T>(&self, pid: nix::unistd::Pid, signal: T) -> nix::Result<()>
    where
        T: Into<Option<nix::sys::signal::Signal>> + Send;
}

pub type DynProcessManager = Arc<dyn Process + Send + Sync>;
