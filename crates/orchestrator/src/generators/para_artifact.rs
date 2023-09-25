use std::path::{Path, PathBuf};

use provider::{
    types::{GenerateFileCommand, GenerateFilesOptions, TransferedFile},
    DynNamespace,
};
use support::fs::FileSystem;

use super::errors::GeneratorError;
use crate::ScopedFilesystem;

#[derive(Debug, Clone)]
pub(crate) enum ParaArtifactType {
    Wasm,
    State,
}

#[derive(Debug, Clone)]
pub(crate) enum ParaArtifactBuildOption {
    Path(String),
    Command(String),
}

/// Parachain artifact (could be either the genesis state or genesis wasm)
#[derive(Debug, Clone)]
pub struct ParaArtifact {
    artifact_type: ParaArtifactType,
    build_option: ParaArtifactBuildOption,
    artifact_path: Option<PathBuf>,
}

impl ParaArtifact {
    pub(crate) fn new(
        artifact_type: ParaArtifactType,
        build_option: ParaArtifactBuildOption,
    ) -> Self {
        Self {
            artifact_type,
            build_option,
            artifact_path: None,
        }
    }

    pub(crate) fn artifact_path(&self) -> Option<&PathBuf> {
        self.artifact_path.as_ref()
    }

    pub(crate) async fn build<'a, T>(
        &mut self,
        chain_spec_path: Option<impl AsRef<Path>>,
        artifact_path: impl AsRef<Path>,
        ns: &DynNamespace,
        scoped_fs: &ScopedFilesystem<'a, T>,
    ) -> Result<(), GeneratorError>
    where
        T: FileSystem,
    {
        match &self.build_option {
            ParaArtifactBuildOption::Path(path) => {
                let t = TransferedFile {
                    local_path: PathBuf::from(path),
                    remote_path: artifact_path.as_ref().into(),
                };
                scoped_fs.copy_files(vec![&t]).await?;
            },
            ParaArtifactBuildOption::Command(cmd) => {
                let generate_subcmd = match self.artifact_type {
                    ParaArtifactType::Wasm => "export-genesis-wasm",
                    ParaArtifactType::State => "export-genesis-state",
                };

                let mut args: Vec<String> = vec![generate_subcmd.into()];
                if let Some(chain_spec_path) = chain_spec_path {
                    let full_chain_path = format!(
                        "{}/{}",
                        ns.base_dir(),
                        chain_spec_path.as_ref().to_string_lossy()
                    );
                    args.push("--chain".into());
                    args.push(full_chain_path)
                }

                // TODO: add to logger
                // println!("{:#?}", &args);
                let artifact_path_ref = artifact_path.as_ref();
                let generate_command =
                    GenerateFileCommand::new(cmd.as_str(), artifact_path_ref).args(args);
                let options = GenerateFilesOptions::new(vec![generate_command]);
                ns.generate_files(options).await?;
                self.artifact_path = Some(artifact_path_ref.into());
            },
        }

        Ok(())
    }
}
