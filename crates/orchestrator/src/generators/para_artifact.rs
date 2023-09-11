use std::path::PathBuf;

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

    pub(crate) async fn build(&mut self) -> Result<(), ()> {
        todo!()
    }
}
