use std::{error::Error, marker::PhantomData};

use serde::{Deserialize, Serialize};

use crate::{
    shared::{
        errors::FieldError,
        helpers::{ensure_value_is_not_empty, merge_errors, sanitize_node_name},
        macros::states,
        node::EnvVar,
        resources::{Resources, ResourcesBuilder},
    },
    types::{Arg, Command, Image, Port},
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

/// A port a custom process listens on, by name.
///
/// The name is what the port is exposed under (a k8s `Service` port, the key
/// the running process reports its addresses by), so it has to be a valid
/// service name: 1 to 15 lowercase letters, digits or `-`, with a letter in
/// it. `port` is the number the process listens on; `0` lets zombienet pick a
/// free one at spawn time, handed to the process through
/// `{{ZOMBIE:<process>:port_<name>}}` in its args or env.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedPort {
    pub name: String,
    pub port: Port,
}

/// k8s' rule for a named port (IANA_SVC_NAME), which is the strictest of the
/// places a port name ends up.
fn validate_port_name(name: &str) -> Result<(), anyhow::Error> {
    if name.is_empty() || name.len() > 15 {
        return Err(anyhow::anyhow!(
            "port name '{name}' must be 1 to 15 characters long"
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(anyhow::anyhow!(
            "port name '{name}' may only contain lowercase letters, digits and '-'"
        ));
    }
    if !name.chars().any(|c| c.is_ascii_lowercase()) {
        return Err(anyhow::anyhow!(
            "port name '{name}' must contain at least one letter"
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(anyhow::anyhow!(
            "port name '{name}' may not start or end with '-' or contain '--'"
        ));
    }
    Ok(())
}

/// Represent a custom process to spawn, allowing to set:
/// cmd: Command to execute
/// args: Argumnets to pass
/// env: Environment to set
/// image: Image to use (provider specific)
/// ports: Named ports the process listens on
/// resources: Resources to apply (provider specific)
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
    // Named ports the process listens on
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    ports: Vec<NamedPort>,
    // Resources to apply (only k8s)
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<Resources>,
}

impl Default for CustomProcess {
    fn default() -> Self {
        Self {
            name: "".into(),
            image: None,
            command: Command::default(), // should be changed.
            args: vec![],
            env: vec![],
            ports: vec![],
            resources: None,
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

    /// Named ports the process listens on.
    pub fn ports(&self) -> &[NamedPort] {
        &self.ports
    }

    /// Resources to apply to the process (only k8s).
    pub fn resources(&self) -> Option<&Resources> {
        self.resources.as_ref()
    }

    /// The field rules, applied by the builder and by the TOML loader: the name
    /// is an RFC 1035 label like a node's (rejected rather than corrected, it
    /// is the lookup key), port names are valid service names and unique, and
    /// no two ports share a number (`0` excepted, each is picked separately).
    pub fn validate(&self) -> Result<(), Vec<anyhow::Error>> {
        let mut errors = vec![];
        let valid_name = sanitize_node_name(&self.name);
        if self.name.is_empty() {
            errors.push(FieldError::Name(anyhow::anyhow!("can't be empty")).into());
        } else if valid_name != self.name {
            errors.push(
                FieldError::Name(anyhow::anyhow!(
                    "'{}' must be lowercase letters, digits and '-', starting with a letter and at most 63 characters (e.g. '{valid_name}')",
                    self.name
                ))
                .into(),
            );
        }
        let mut names: Vec<&str> = vec![];
        let mut numbers: Vec<Port> = vec![];
        for port in &self.ports {
            if let Err(e) = validate_port_name(&port.name) {
                errors.push(FieldError::Ports(e).into());
            }
            if names.contains(&port.name.as_str()) {
                errors.push(
                    FieldError::Ports(anyhow::anyhow!(
                        "port name '{}' is declared twice",
                        port.name
                    ))
                    .into(),
                );
            }
            if port.port != 0 && numbers.contains(&port.port) {
                errors.push(
                    FieldError::Ports(anyhow::anyhow!(
                        "port {} is declared twice (as '{}')",
                        port.port,
                        port.name
                    ))
                    .into(),
                );
            }
            names.push(&port.name);
            numbers.push(port.port);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
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

    /// Declare a named port the process listens on; `0` lets zombienet pick a
    /// free one at spawn time. Names and numbers are checked in [`Self::build`].
    pub fn with_port(self, name: impl Into<String>, port: Port) -> Self {
        let mut ports = self.config.ports;
        ports.push(NamedPort {
            name: name.into(),
            port,
        });

        Self::transition(
            CustomProcess {
                ports,
                ..self.config
            },
            self.errors,
        )
    }

    /// Set the resources to apply to the process (only k8s).
    pub fn with_resources(self, f: impl FnOnce(ResourcesBuilder) -> ResourcesBuilder) -> Self {
        match f(ResourcesBuilder::new()).build() {
            Ok(resources) => Self::transition(
                CustomProcess {
                    resources: Some(resources),
                    ..self.config
                },
                self.errors,
            ),
            Err(errors) => Self::transition(
                self.config,
                errors.into_iter().fold(self.errors, |acc, e| {
                    merge_errors(acc, FieldError::Resources(e).into())
                }),
            ),
        }
    }

    /// Seals the builder and returns a [`CustomProcess`] if there are no validation errors, else returns errors.
    pub fn build(self) -> Result<CustomProcess, (String, Vec<anyhow::Error>)> {
        let mut errors = self.errors;
        if let Err(port_errors) = self.config.validate() {
            errors.extend(port_errors);
        }

        if !errors.is_empty() {
            return Err((self.config.name.clone(), errors));
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
        assert!(cpb.ports().is_empty());
        assert!(cpb.resources().is_none());
    }

    #[test]
    fn ports_and_resources_are_carried_and_round_trip() {
        let cp = CustomProcessBuilder::new()
            .with_name("ipfs")
            .with_command("ipfs")
            .with_image("docker.io/ipfs/kubo:v0.39.0")
            .with_port("api", 5001)
            .with_port("gateway", 8080)
            .with_resources(|r| r.with_limit_cpu("500m").with_limit_memory("512Mi"))
            .build()
            .unwrap();

        assert_eq!(
            cp.ports(),
            &[
                NamedPort {
                    name: "api".into(),
                    port: 5001
                },
                NamedPort {
                    name: "gateway".into(),
                    port: 8080
                }
            ]
        );
        assert_eq!(
            cp.resources().unwrap().limit_cpu().unwrap().as_str(),
            "500m"
        );

        let json = serde_json::to_value(&cp).unwrap();
        assert_eq!(json["ports"][1]["name"], "gateway");
        assert_eq!(json["ports"][1]["port"], 8080);
        let back: CustomProcess = serde_json::from_value(json).unwrap();
        assert_eq!(back, cp);
    }

    #[test]
    fn a_process_without_ports_serializes_as_before() {
        let cp = CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .build()
            .unwrap();
        let json = serde_json::to_value(&cp).unwrap();
        assert!(json.get("ports").is_none());
        assert!(json.get("resources").is_none());
    }

    #[test]
    fn duplicate_port_names_and_numbers_are_rejected() {
        let err = CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .with_port("api", 5001)
            .with_port("api", 5002)
            .build()
            .unwrap_err();
        assert_eq!(err.0, "demo");
        assert!(err.1[0].to_string().contains("declared twice"));

        let err = CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .with_port("api", 5001)
            .with_port("other", 5001)
            .build()
            .unwrap_err();
        assert!(err.1[0].to_string().contains("port 5001 is declared twice"));

        // Two ports left for zombienet to pick never collide.
        CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .with_port("api", 0)
            .with_port("gateway", 0)
            .build()
            .unwrap();
    }

    #[test]
    fn port_names_must_be_valid_service_names() {
        for bad in [
            "",
            "json_rpc",
            "Gateway",
            "a-name-that-is-too-long",
            "8080",
            "-api",
            "api--x",
        ] {
            let err = CustomProcessBuilder::new()
                .with_name("demo")
                .with_command("some")
                .with_port(bad, 5001)
                .build()
                .unwrap_err();
            assert!(
                err.1[0].to_string().starts_with("ports: port name"),
                "{bad:?} should be rejected, got {}",
                err.1[0]
            );
        }
        for good in ["api", "rpc-http", "p2p", "a", "x-15-chars-long"] {
            CustomProcessBuilder::new()
                .with_name("demo")
                .with_command("some")
                .with_port(good, 5001)
                .build()
                .unwrap_or_else(|e| panic!("{good:?} should be accepted, got {:?}", e.1));
        }
    }
}
