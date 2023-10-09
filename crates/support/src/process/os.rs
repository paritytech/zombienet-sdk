use std::{io, sync::Arc};

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::DynAsyncRead;
use crate::process::{Command, DynProcess, Process, ProcessManager};

#[derive(Debug)]
struct OsProcess {
    child: RwLock<tokio::process::Child>,
}

#[async_trait]
impl Process for OsProcess {
    async fn id(&self) -> Option<u32> {
        self.child.read().await.id()
    }

    async fn take_stdout(&self) -> Option<DynAsyncRead> {
        self.child
            .write()
            .await
            .stdout
            .take()
            .map(|stdout| Box::new(stdout) as DynAsyncRead)
    }

    async fn take_stderr(&self) -> Option<DynAsyncRead> {
        self.child
            .write()
            .await
            .stderr
            .take()
            .map(|stderr| Box::new(stderr) as DynAsyncRead)
    }

    async fn kill(&self) -> io::Result<()> {
        self.child.write().await.kill().await
    }
}

#[derive(Clone)]
pub struct OsProcessManager;

impl OsProcessManager {
    fn create_base_command(command: Command) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(command.program.clone());

        if !command.args.is_empty() {
            cmd.args(command.args.clone());
        }

        if !command.envs.is_empty() {
            cmd.envs(command.envs.clone());
        }

        if let Some(stdin) = command.stdin {
            cmd.stdin(stdin);
        }

        if let Some(stdout) = command.stdout {
            cmd.stdout(stdout);
        }

        if let Some(stderr) = command.stderr {
            cmd.stderr(stderr);
        }

        cmd
    }
}

#[async_trait]
impl ProcessManager for OsProcessManager {
    fn spawn(&self, command: Command) -> io::Result<DynProcess> {
        let kill_on_drop = command.kill_on_drop;
        let mut base_command = OsProcessManager::create_base_command(command);

        if kill_on_drop {
            base_command.kill_on_drop(true);
        }

        Ok(base_command.spawn().map(|child| {
            Arc::new(OsProcess {
                child: RwLock::new(child),
            })
        })?)
    }

    async fn output(&self, command: Command) -> io::Result<std::process::Output> {
        let mut base_command = OsProcessManager::create_base_command(command);

        Ok(base_command.output().await?)
    }

    fn kill<T>(&self, pid: nix::unistd::Pid, signal: T) -> nix::Result<()>
    where
        T: Into<Option<nix::sys::signal::Signal>>,
    {
        nix::sys::signal::kill(pid, signal)
    }
}
