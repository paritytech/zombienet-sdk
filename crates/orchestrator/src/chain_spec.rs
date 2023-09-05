use std::path::{PathBuf, Path};

use provider::Provider;

#[derive(Debug, Clone)]
pub(crate) struct ChainSpec {
    maybe_plain_path: Option<PathBuf>,
    chain_name: Option<String>,
    raw_path: Option<PathBuf>,
    build_command: Option<String>,
}

impl ChainSpec {
    pub fn new(chain_name: impl Into<String>, command: impl Into<String>) -> Self {
        let chain_name = chain_name.into();
        let build_command = format!(
            "{} build-spec --chain {} --disable-default-bootnode",
            command.into(),
            &chain_name
        );

        Self {
            build_command: Some(build_command),
            chain_name: Some(chain_name.into()),
            maybe_plain_path: None,
            raw_path: None,
        }
    }

    pub fn new_with_path(chain_name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        let chain_name = chain_name.into();

        Self {
            build_command: None,
            chain_name: Some(chain_name.into()),
            maybe_plain_path: Some(path.into()),
            raw_path: None,
        }
    }

    pub async fn build(&mut self, _provider: &impl Provider) -> Result<(), ()> {
        // create a temp node
        todo!()
    }

    pub fn is_raw(&self) -> bool {
        todo!()
    }

    pub fn raw_path(&self) -> Option<&Path> {
        self.raw_path.as_deref()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
}
