use std::{error::Error, marker::PhantomData};

use serde::{Deserialize, Serialize};

use crate::{
    shared::{
        errors::FieldError,
        helpers::{ensure_value_is_not_empty, merge_errors},
        macros::states,
        node::EnvVar,
    },
    types::{Arg, Command, Image},
};

states! {
    WithName,
    WithOutName
}

states! {
    WithCmd,
    WithOutCmd
}

pub trait Cmd {}
impl Cmd for WithOutCmd {}
impl Cmd for WithCmd {}

/// Represent a custom process to spawn, allowing to set:
/// cmd: Command to execute
/// args: Argumnets to pass
/// env: Environment to set
/// image: Image to use (provider specific)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomProcess {
    // Name of the process
    name: String,
    // Image to use
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<Image>,
    // Command to execute
    command: Command,
    // Arguments to pass
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    args: Vec<Arg>,
    // Environment to set
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    env: Vec<EnvVar>,
}

impl Default for CustomProcess {
    fn default() -> Self {
        Self {
            name: "".into(),
            image: None,
            command: Command::default(), // should be changed.
            args: vec![],
            env: vec![],
        }
    }
}

impl CustomProcess {
    /// Node name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Image to run (only podman/k8s).
    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    /// Command to run the node.
    pub fn command(&self) -> &Command {
        &self.command
    }

    /// Arguments to use for node.
    pub fn args(&self) -> Vec<&Arg> {
        self.args.iter().collect()
    }

    /// Environment variables to set (inside pod for podman/k8s, inside shell for native).
    pub fn env(&self) -> Vec<&EnvVar> {
        self.env.iter().collect()
    }
}

/// A node configuration builder, used to build a [`NodeConfig`] declaratively with fields validation.
pub struct CustomProcessBuilder<N, C> {
    config: CustomProcess,
    errors: Vec<anyhow::Error>,
    _state_name: PhantomData<N>,
    _state_cmd: PhantomData<C>,
}

impl Default for CustomProcessBuilder<WithOutName, WithOutCmd> {
    fn default() -> Self {
        Self {
            config: CustomProcess::default(),
            errors: vec![],
            _state_name: PhantomData,
            _state_cmd: PhantomData,
        }
    }
}

impl<A, B> CustomProcessBuilder<A, B> {
    fn transition<C, D>(
        config: CustomProcess,
        errors: Vec<anyhow::Error>,
    ) -> CustomProcessBuilder<C, D> {
        CustomProcessBuilder {
            config,
            errors,
            _state_name: PhantomData,
            _state_cmd: PhantomData,
        }
    }
}

impl CustomProcessBuilder<WithOutName, WithOutCmd> {
    pub fn new() -> CustomProcessBuilder<WithOutName, WithOutCmd> {
        CustomProcessBuilder::default()
    }
}

impl<C: Cmd> CustomProcessBuilder<WithOutName, C> {
    /// set the name of the process.
    pub fn with_name<T: Into<String> + Copy>(self, name: T) -> CustomProcessBuilder<WithName, C> {
        let name: String = name.into();

        match ensure_value_is_not_empty(&name) {
            Ok(_) => Self::transition(
                CustomProcess {
                    name,
                    ..self.config
                },
                self.errors,
            ),
            Err(e) => Self::transition(
                CustomProcess {
                    // we still set the name in error case to display error path
                    name,
                    ..self.config
                },
                merge_errors(self.errors, FieldError::Name(e).into()),
            ),
        }
    }
}

impl CustomProcessBuilder<WithName, WithOutCmd> {
    /// Set the command that will be executed to spawn the process.
    pub fn with_command<T>(self, command: T) -> CustomProcessBuilder<WithName, WithCmd>
    where
        T: TryInto<Command>,
        T::Error: Error + Send + Sync + 'static,
    {
        match command.try_into() {
            Ok(command) => Self::transition(
                CustomProcess {
                    command,
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::Command(error.into()).into()),
            ),
        }
    }
}

impl CustomProcessBuilder<WithName, WithCmd> {
    /// Set the image that will be used for the node (only podman/k8s).
    pub fn with_image<T>(self, image: T) -> Self
    where
        T: TryInto<Image>,
        T::Error: Error + Send + Sync + 'static,
    {
        match image.try_into() {
            Ok(image) => Self::transition(
                CustomProcess {
                    image: Some(image),
                    ..self.config
                },
                self.errors,
            ),
            Err(error) => Self::transition(
                self.config,
                merge_errors(self.errors, FieldError::Image(error.into()).into()),
            ),
        }
    }

    /// Set the arguments that will be used when spawn the process.
    pub fn with_args(self, args: Vec<Arg>) -> Self {
        Self::transition(
            CustomProcess {
                args,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the  environment variables that will be used when spawn the process.
    pub fn with_env(self, env: Vec<impl Into<EnvVar>>) -> Self {
        let env = env.into_iter().map(|var| var.into()).collect::<Vec<_>>();

        Self::transition(CustomProcess { env, ..self.config }, self.errors)
    }

    /// Seals the builder and returns a [`CustomProcess`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<CustomProcess, (String, Vec<anyhow::Error>)> {
        if !self.errors.is_empty() {
            return Err((self.config.name.clone(), self.errors));
        }

        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_process_config_builder_should_succeeds_and_returns_a_custom_process_config() {
        let cpb = CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .with_args(vec![("--port", "100").into(), "--custom-flag".into()])
            .build()
            .unwrap();

        assert_eq!(cpb.command().as_str(), "some");
        let args: Vec<Arg> = vec![("--port", "100").into(), "--custom-flag".into()];
        assert_eq!(cpb.args(), args.iter().collect::<Vec<&Arg>>());
    }
}
