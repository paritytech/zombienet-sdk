use std::path::{Path, PathBuf};

use provider::{
    constants::NODE_CONFIG_DIR,
    types::{GenerateFileCommand, GenerateFilesOptions, TransferedFile},
    DynNamespace,
};
use support::fs::FileSystem;
use uuid::Uuid;

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
    // image to use for building the para artifact
    image: Option<String>,
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
            image: None,
        }
    }

    pub(crate) fn image(mut self, image: Option<String>) -> Self {
        self.image = image;
        self
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
                let t = TransferedFile::new(PathBuf::from(path), artifact_path.as_ref().into());
                scoped_fs.copy_files(vec![&t]).await?;
                self.artifact_path = Some(artifact_path.as_ref().into());
            },
            ParaArtifactBuildOption::Command(cmd) => {
                let generate_subcmd = match self.artifact_type {
                    ParaArtifactType::Wasm => "export-genesis-wasm",
                    ParaArtifactType::State => "export-genesis-state",
                };

                // TODO: replace uuid with para_id-random
                let temp_name = format!("temp-{}-{}", generate_subcmd, Uuid::new_v4());
                let mut args: Vec<String> = vec![generate_subcmd.into()];

                let files_to_inject = if let Some(chain_spec_path) = chain_spec_path {
                    // TODO: we should get the full path from the scoped filesystem
                    let chain_spec_path_local = format!(
                        "{}/{}",
                        ns.base_dir().to_string_lossy(),
                        chain_spec_path.as_ref().to_string_lossy()
                    );
                    // Remote path to be injected
                    let chain_spec_path_in_pod = format!(
                        "{}/{}",
                        NODE_CONFIG_DIR,
                        chain_spec_path.as_ref().to_string_lossy()
                    );
                    // Path in the context of the node, this can be different in the context of the providers (e.g native)
                    let chain_spec_path_in_args = if ns.capabilities().prefix_with_full_path {
                        // In native
                        format!(
                            "{}/{}{}",
                            ns.base_dir().to_string_lossy(),
                            &temp_name,
                            &chain_spec_path_in_pod
                        )
                    } else {
                        chain_spec_path_in_pod.clone()
                    };

                    args.push("--chain".into());
                    args.push(chain_spec_path_in_args);

                    vec![TransferedFile::new(
                        chain_spec_path_local,
                        chain_spec_path_in_pod,
                    )]
                } else {
                    vec![]
                };

                let artifact_path_ref = artifact_path.as_ref();
                let generate_command =
                    GenerateFileCommand::new(cmd.as_str(), artifact_path_ref).args(args);
                let options = GenerateFilesOptions::with_files(
                    vec![generate_command],
                    self.image.clone(),
                    &files_to_inject,
                )
                .temp_name(temp_name);
                ns.generate_files(options).await?;
                self.artifact_path = Some(artifact_path_ref.into());
            },
        }

        Ok(())
    }
}
