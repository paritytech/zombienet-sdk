use std::path::{Path, PathBuf};

pub type Port = u16;

#[derive(Debug, Default, Clone)]
pub struct ProviderCapabilities {
    pub requires_image: bool,
}

impl ProviderCapabilities {
    fn new() -> Self {
        Self::default()
    }

    fn requires_image(mut self) -> Self {
        self.requires_image = true;
        self
    }
}

pub struct SpawnNodeOptions {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub injected_files: Vec<TransferedFile>,
}

impl SpawnNodeOptions {
    fn new<S>(name: S, command: S) -> Self
    where
        S: AsRef<str>,
    {
        Self {
            name: name.as_ref().to_string(),
            command: command.as_ref().to_string(),
            args: vec![],
            env: vec![],
            injected_files: vec![],
        }
    }

    fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    fn env<S, I>(mut self, env: I) -> Self
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

pub struct GenerateFileCommand {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub local_output_path: PathBuf,
}

impl GenerateFileCommand {
    fn new<S, P>(command: S, local_output_path: P) -> Self
    where
        S: AsRef<str>,
        P: AsRef<Path>,
    {
        Self {
            command: command.as_ref().to_string(),
            args: vec![],
            env: vec![],
            local_output_path: local_output_path.as_ref().into(),
        }
    }

    fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    fn env<S, I>(mut self, env: I) -> Self
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

pub struct GenerateFilesOptions {
    pub commands: Vec<GenerateFileCommand>,
    pub injected_files: Vec<TransferedFile>,
}

impl GenerateFilesOptions {
    fn new<I>(commands: I) -> Self
    where
        I: IntoIterator<Item = GenerateFileCommand>,
    {
        Self {
            commands: commands.into_iter().collect(),
            injected_files: vec![],
        }
    }

    fn injected_files<I>(mut self, injected_files: I) -> Self
    where
        I: IntoIterator<Item = TransferedFile>,
    {
        self.injected_files = injected_files.into_iter().collect();
        self
    }
}

pub struct RunCommandOptions {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl RunCommandOptions {
    fn new<S>(command: S) -> Self
    where
        S: AsRef<str>,
    {
        Self {
            command: command.as_ref().to_string(),
            args: vec![],
            env: vec![],
        }
    }

    fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    fn env<S, I>(mut self, env: I) -> Self
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
    fn new<P>(local_script_path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            local_script_path: local_script_path.as_ref().into(),
            args: vec![],
            env: vec![],
        }
    }

    fn args<S, I>(mut self, args: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        self.args = args.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    fn env<S, I>(mut self, env: I) -> Self
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

pub struct TransferedFile {
    pub local_path: PathBuf,
    pub remote_path: PathBuf,
}

impl TransferedFile {
    fn new<P>(local_path: P, remote_path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            local_path: local_path.as_ref().into(),
            remote_path: remote_path.as_ref().into(),
        }
    }
}
