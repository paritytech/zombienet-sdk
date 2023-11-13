use std::{
    path::{Path, PathBuf},
    process::ExitStatus,
};

use configuration::shared::resources::Resources;

pub type Port = u16;

pub type ExecutionResult = Result<String, (ExitStatus, String)>;

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCapabilities {
    pub requires_image: bool,
    pub has_resources: bool,
}

pub struct SpawnNodeOptions {
    pub name: String,
    pub image: Option<String>,
    pub resources: Option<Resources>,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    // TODO: naming
    pub injected_files: Vec<TransferedFile>,
    /// Paths to create before start the node (e.g keystore)
    /// should be created with `create_dir_all` in order
    /// to create the full path even when we have missing parts
    pub created_paths: Vec<PathBuf>,
}

impl SpawnNodeOptions {
    pub fn new<S>(name: S, program: S) -> Self
    where
        S: AsRef<str>,
    {
        Self {
            name: name.as_ref().to_string(),
            image: None,
            resources: None,
            program: program.as_ref().to_string(),
            args: vec![],
            env: vec![],
            injected_files: vec![],
            created_paths: vec![],
        }
    }

    pub fn image<S>(mut self, image: S) -> Self
    where
        S: AsRef<str>,
    {
        self.image = Some(image.as_ref().to_string());
        self
    }

    pub fn resources(mut self, resources: Resources) -> Self {
        self.resources = Some(resources);
        self
    }

    pub fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    pub fn env<S, I>(mut self, env: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = (S, S)>,
    {
        self.env = env
            .into_iter()
            .map(|(name, value)| (name.as_ref().to_string(), value.as_ref().to_string()))
            .collect();
        self
    }

    pub fn injected_files<I>(mut self, injected_files: I) -> Self
    where
        I: IntoIterator<Item = TransferedFile>,
    {
        self.injected_files = injected_files.into_iter().collect();
        self
    }

    pub fn created_paths<P, I>(mut self, created_paths: I) -> Self
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        self.created_paths = created_paths
            .into_iter()
            .map(|path| path.as_ref().into())
            .collect();
        self
    }
}

#[derive(Debug)]
pub struct GenerateFileCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub local_output_path: PathBuf,
}

impl GenerateFileCommand {
    pub fn new<S, P>(program: S, local_output_path: P) -> Self
    where
        S: AsRef<str>,
        P: AsRef<Path>,
    {
        Self {
            program: program.as_ref().to_string(),
            args: vec![],
            env: vec![],
            local_output_path: local_output_path.as_ref().into(),
        }
    }

    pub fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    pub fn env<S, I>(mut self, env: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = (S, S)>,
    {
        self.env = env
            .into_iter()
            .map(|(name, value)| (name.as_ref().to_string(), value.as_ref().to_string()))
            .collect();
        self
    }
}

#[derive(Debug)]
pub struct GenerateFilesOptions {
    pub commands: Vec<GenerateFileCommand>,
    pub injected_files: Vec<TransferedFile>,
}

impl GenerateFilesOptions {
    pub fn new<I>(commands: I) -> Self
    where
        I: IntoIterator<Item = GenerateFileCommand>,
    {
        Self {
            commands: commands.into_iter().collect(),
            injected_files: vec![],
        }
    }

    pub fn injected_files<I>(mut self, injected_files: I) -> Self
    where
        I: IntoIterator<Item = TransferedFile>,
    {
        self.injected_files = injected_files.into_iter().collect();
        self
    }
}

pub struct RunCommandOptions {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl RunCommandOptions {
    pub fn new<S>(program: S) -> Self
    where
        S: AsRef<str>,
    {
        Self {
            program: program.as_ref().to_string(),
            args: vec![],
            env: vec![],
        }
    }

    pub fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    pub fn env<S, I>(mut self, env: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = (S, S)>,
    {
        self.env = env
            .into_iter()
            .map(|(name, value)| (name.as_ref().to_string(), value.as_ref().to_string()))
            .collect();
        self
    }
}

pub struct RunScriptOptions {
    pub local_script_path: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl RunScriptOptions {
    pub fn new<P>(local_script_path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            local_script_path: local_script_path.as_ref().into(),
            args: vec![],
            env: vec![],
        }
    }

    pub fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    pub fn env<S, I>(mut self, env: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = (S, S)>,
    {
        self.env = env
            .into_iter()
            .map(|(name, value)| (name.as_ref().to_string(), value.as_ref().to_string()))
            .collect();
        self
    }
}

// TODO(team): I think we can rename it to FileMap?
#[derive(Debug, Clone)]
pub struct TransferedFile {
    pub local_path: PathBuf,
    pub remote_path: PathBuf,
    // TODO: Can be narrowed to have strict typing on this?
    pub mode: String,
}

impl TransferedFile {
    pub fn new<P>(local_path: P, remote_path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            local_path: local_path.as_ref().into(),
            remote_path: remote_path.as_ref().into(),
            mode: "0644".to_string(), // default to rw-r--r--
        }
    }

    pub fn mode<S>(mut self, mode: S) -> Self
    where
        S: AsRef<str>,
    {
        self.mode = mode.as_ref().to_string();
        self
    }
}

impl std::fmt::Display for TransferedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "File to transfer (local: {}, remote: {})",
            self.local_path.display(),
            self.remote_path.display()
        )
    }
}
