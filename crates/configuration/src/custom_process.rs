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
    utils::is_false,
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

/// How a custom process is known to be ready. `tcp` is a declared port name
/// that must accept a connection; `http` is a declared port name and a path
/// that must answer 2xx. On kubernetes the check is the pod's readiness
/// probe; elsewhere zombienet probes the port from where it runs. How long
/// to wait is the process's `timeout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyCheck {
    #[serde(flatten)]
    pub target: ReadyTarget,
    /// Seconds between two attempts.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub interval: u32,
}

/// What a [`ReadyCheck`] probes, by declared port name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadyTarget {
    /// The named port accepts a TCP connection.
    Tcp(String),
    /// `GET <path>` on the named port answers 2xx.
    Http { port: String, path: String },
}

impl ReadyTarget {
    /// The name of the port the check probes.
    pub fn port_name(&self) -> &str {
        match self {
            Self::Tcp(port) | Self::Http { port, .. } => port,
        }
    }

    /// The path a `GET` must answer 2xx on; `None` for a TCP connect.
    pub fn http_path(&self) -> Option<&str> {
        match self {
            Self::Tcp(_) => None,
            Self::Http { path, .. } => Some(path),
        }
    }
}

fn one() -> u32 {
    1
}

fn is_one(value: &u32) -> bool {
    *value == 1
}

impl ReadyCheck {
    /// A check that passes once the port named `port` accepts a connection.
    pub fn tcp(port: impl Into<String>) -> Self {
        Self {
            target: ReadyTarget::Tcp(port.into()),
            interval: 1,
        }
    }

    /// A check that passes once `GET <path>` on the port named `port` answers 2xx.
    pub fn http(port: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            target: ReadyTarget::Http {
                port: port.into(),
                path: path.into(),
            },
            interval: 1,
        }
    }

    pub fn with_interval(mut self, secs: u32) -> Self {
        self.interval = secs;
        self
    }

    /// The name of the port the check probes.
    pub fn port_name(&self) -> &str {
        self.target.port_name()
    }
}

/// When a dependency counts as met, as docker-compose names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCondition {
    /// The process started.
    ServiceStarted,
    /// The process passed its ready check.
    ServiceHealthy,
    /// The one-shot process exited 0.
    ServiceCompletedSuccessfully,
}

impl DependencyCondition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ServiceStarted => "service_started",
            Self::ServiceHealthy => "service_healthy",
            Self::ServiceCompletedSuccessfully => "service_completed_successfully",
        }
    }
}

/// Another custom process this one starts after. Written as a bare name
/// (`"postgres"`) or as `{ name = "init", condition = "..." }`; without a
/// condition the target's natural one applies, see
/// [`CustomProcess::natural_condition`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "DependencyRepr", into = "DependencyRepr")]
pub struct Dependency {
    pub name: String,
    pub condition: Option<DependencyCondition>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum DependencyRepr {
    Name(String),
    Full {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        condition: Option<DependencyCondition>,
    },
}

impl From<DependencyRepr> for Dependency {
    fn from(repr: DependencyRepr) -> Self {
        match repr {
            DependencyRepr::Name(name) => Self {
                name,
                condition: None,
            },
            DependencyRepr::Full { name, condition } => Self { name, condition },
        }
    }
}

impl From<Dependency> for DependencyRepr {
    fn from(dep: Dependency) -> Self {
        match dep.condition {
            None => Self::Name(dep.name),
            Some(condition) => Self::Full {
                name: dep.name,
                condition: Some(condition),
            },
        }
    }
}

impl Dependency {
    /// The condition this dependency means on `target`: the one named, or
    /// the target's natural one.
    pub fn condition_on(&self, target: &CustomProcess) -> DependencyCondition {
        self.condition.unwrap_or_else(|| target.natural_condition())
    }
}

impl From<&str> for Dependency {
    fn from(name: &str) -> Self {
        Self {
            name: name.into(),
            condition: None,
        }
    }
}

/// The longest `timeout` accepted: a week, in seconds.
pub const MAX_TIMEOUT_SECS: u32 = 7 * 24 * 60 * 60;

/// A custom process to spawn next to the nodes: its `command`, `args`, `env`,
/// `image` (provider specific), named `ports`, `resources` (provider
/// specific), and its lifecycle: `ready_check`, `depends_on`, `one_shot` and
/// `timeout`.
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
    // How the process is known to be ready
    #[serde(skip_serializing_if = "Option::is_none")]
    ready_check: Option<ReadyCheck>,
    // Processes this one starts after
    #[serde(skip_serializing_if = "std::vec::Vec::is_empty", default)]
    depends_on: Vec<Dependency>,
    // Runs to completion instead of staying up
    #[serde(skip_serializing_if = "is_false", default)]
    one_shot: bool,
    // Seconds to wait for the condition: the ready check, or a one-shot's exit
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<u32>,
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
            ready_check: None,
            depends_on: vec![],
            one_shot: false,
            timeout: None,
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

    /// How the process is known to be ready, if declared. Without one, a
    /// process with ports is ready when its first port accepts a connection,
    /// and a process without ports is ready once started.
    pub fn ready_check(&self) -> Option<&ReadyCheck> {
        self.ready_check.as_ref()
    }

    /// The processes this one starts after.
    pub fn depends_on(&self) -> &[Dependency] {
        &self.depends_on
    }

    /// Whether the process runs to completion (exit 0 is success) instead of
    /// staying up.
    pub fn one_shot(&self) -> bool {
        self.one_shot
    }

    /// Seconds to wait for the process to meet its condition (the ready check
    /// to pass, or a one-shot to exit); the global `node_spawn_timeout` when
    /// absent.
    pub fn timeout(&self) -> Option<u32> {
        self.timeout
    }

    /// What the process is waited on: the `ready_check`, or a TCP connect on
    /// the first declared port when there is none. `None` for a one-shot
    /// (waited on to exit) and for a process with nothing to check (ready
    /// once started).
    pub fn effective_check(&self) -> Option<ReadyTarget> {
        if self.one_shot {
            return None;
        }
        match &self.ready_check {
            Some(check) => Some(check.target.clone()),
            None => self.ports.first().map(|p| ReadyTarget::Tcp(p.name.clone())),
        }
    }

    /// The condition a dependency on this process means when it names none:
    /// completed for a one-shot, healthy when there is something to check,
    /// started otherwise.
    pub fn natural_condition(&self) -> DependencyCondition {
        if self.one_shot {
            DependencyCondition::ServiceCompletedSuccessfully
        } else if self.effective_check().is_some() {
            DependencyCondition::ServiceHealthy
        } else {
            DependencyCondition::ServiceStarted
        }
    }

    /// The field rules shared by the builder and the TOML loader. What is
    /// checked at spawn follows from them, see [`Self::effective_check`].
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

        if let Some(check) = &self.ready_check {
            if !names.contains(&check.port_name()) {
                errors.push(
                    FieldError::ReadyCheck(anyhow::anyhow!(
                        "port '{}' is not declared in ports",
                        check.port_name()
                    ))
                    .into(),
                );
            }
            if self.one_shot {
                errors.push(
                    FieldError::ReadyCheck(anyhow::anyhow!(
                        "a one-shot process runs to completion, it has no readiness"
                    ))
                    .into(),
                );
            }
            if let Some(path) = check.target.http_path() {
                if !path.starts_with('/') {
                    errors.push(
                        FieldError::ReadyCheck(anyhow::anyhow!(
                            "http path '{path}' must start with '/'"
                        ))
                        .into(),
                    );
                }
            }
        }
        if self.ready_check.as_ref().is_some_and(|c| c.interval == 0) {
            errors.push(
                FieldError::ReadyCheck(anyhow::anyhow!("interval must be at least 1 second"))
                    .into(),
            );
        }
        match self.timeout {
            Some(0) => errors
                .push(FieldError::Timeout(anyhow::anyhow!("must be at least 1 second")).into()),
            Some(secs) if secs > MAX_TIMEOUT_SECS => errors.push(
                FieldError::Timeout(anyhow::anyhow!(
                    "{secs} is more than the {MAX_TIMEOUT_SECS} seconds (a week) allowed"
                ))
                .into(),
            ),
            _ => {},
        }
        let mut seen: Vec<&str> = vec![];
        for dep in &self.depends_on {
            if dep.name == self.name {
                errors.push(
                    FieldError::DependsOn(anyhow::anyhow!("'{}' depends on itself", dep.name))
                        .into(),
                );
            }
            if seen.contains(&dep.name.as_str()) {
                errors.push(
                    FieldError::DependsOn(anyhow::anyhow!("'{}' is listed twice", dep.name)).into(),
                );
            }
            seen.push(&dep.name);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// The rules between processes, applied once the whole set is known: every
/// dependency names a custom process, its condition fits the target (only a
/// one-shot completes, only a process with something to check is healthy),
/// and there is no cycle. Errors are prefixed with the process they are on.
pub(crate) fn validate_custom_processes(processes: &[CustomProcess]) -> Result<(), Vec<String>> {
    let by_name: std::collections::HashMap<&str, &CustomProcess> =
        processes.iter().map(|p| (p.name(), p)).collect();
    let mut errors = vec![];

    for process in processes {
        for dep in &process.depends_on {
            let Some(target) = by_name.get(dep.name.as_str()) else {
                errors.push(format!(
                    "custom_processes['{}'].depends_on: '{}' is not a custom process",
                    process.name(),
                    dep.name
                ));
                continue;
            };
            let condition = dep.condition_on(target);
            let fits = match condition {
                DependencyCondition::ServiceStarted => true,
                DependencyCondition::ServiceHealthy => target.effective_check().is_some(),
                DependencyCondition::ServiceCompletedSuccessfully => target.one_shot,
            };
            if !fits {
                errors.push(format!(
                    "custom_processes['{}'].depends_on: '{}' cannot be {} ({})",
                    process.name(),
                    dep.name,
                    condition.as_str(),
                    if target.one_shot {
                        "it is a one-shot, which completes"
                    } else if condition == DependencyCondition::ServiceHealthy {
                        "it has no ready_check and no port"
                    } else {
                        "it is not a one-shot"
                    }
                ));
            }
        }
    }

    // A cycle: some process never becomes startable. Peel off the ones whose
    // dependencies are all peeled; what is left is on a cycle.
    let mut remaining: Vec<&CustomProcess> = processes.iter().collect();
    let mut peeled: Vec<&str> = vec![];
    loop {
        let (free, blocked): (Vec<&CustomProcess>, Vec<&CustomProcess>) =
            remaining.iter().partition(|p| {
                p.depends_on.iter().all(|d| {
                    peeled.contains(&d.name.as_str()) || !by_name.contains_key(d.name.as_str())
                })
            });
        if free.is_empty() {
            break;
        }
        peeled.extend(free.iter().map(|p| p.name()));
        remaining = blocked;
    }
    if !remaining.is_empty() {
        let mut names: Vec<&str> = remaining.iter().map(|p| p.name()).collect();
        names.sort();
        errors.push(format!(
            "custom_processes: depends_on forms a cycle among {}",
            names.join(", ")
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// A custom process builder, used to build a [`CustomProcess`] declaratively with fields validation.
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
    pub fn with_named_port(self, name: impl Into<String>, port: Port) -> Self {
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

    /// Declare how the process is known to be ready; see [`ReadyCheck`].
    pub fn with_ready_check(self, check: ReadyCheck) -> Self {
        Self::transition(
            CustomProcess {
                ready_check: Some(check),
                ..self.config
            },
            self.errors,
        )
    }

    /// Start after `name` has met its natural condition (completed for a
    /// one-shot, healthy when it has a check or a port, started otherwise).
    pub fn with_dependency(self, name: impl Into<String>) -> Self {
        self.with_dependency_on(name, None)
    }

    /// Start after `name` has met `condition`, or its natural one when `None`.
    pub fn with_dependency_on(
        self,
        name: impl Into<String>,
        condition: impl Into<Option<DependencyCondition>>,
    ) -> Self {
        let mut depends_on = self.config.depends_on;
        depends_on.push(Dependency {
            name: name.into(),
            condition: condition.into(),
        });
        Self::transition(
            CustomProcess {
                depends_on,
                ..self.config
            },
            self.errors,
        )
    }

    /// Seconds to wait for the process to meet its condition (the ready check
    /// to pass, or a one-shot to exit); the global `node_spawn_timeout`
    /// otherwise.
    pub fn with_timeout(self, secs: u32) -> Self {
        Self::transition(
            CustomProcess {
                timeout: Some(secs),
                ..self.config
            },
            self.errors,
        )
    }

    /// The process runs to completion: exit 0 is success, anything else a
    /// failure; others may depend on it with `service_completed_successfully`.
    pub fn with_one_shot(self) -> Self {
        Self::transition(
            CustomProcess {
                one_shot: true,
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
        if let Err(field_errors) = self.config.validate() {
            errors.extend(field_errors);
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
            .with_named_port("api", 5001)
            .with_named_port("gateway", 8080)
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
            .with_named_port("api", 5001)
            .with_named_port("api", 5002)
            .build()
            .unwrap_err();
        assert_eq!(err.0, "demo");
        assert!(err.1[0].to_string().contains("declared twice"));

        let err = CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .with_named_port("api", 5001)
            .with_named_port("other", 5001)
            .build()
            .unwrap_err();
        assert!(err.1[0].to_string().contains("port 5001 is declared twice"));

        // Two ports left for zombienet to pick never collide.
        CustomProcessBuilder::new()
            .with_name("demo")
            .with_command("some")
            .with_named_port("api", 0)
            .with_named_port("gateway", 0)
            .build()
            .unwrap();
    }

    #[test]
    fn lifecycle_fields_round_trip_and_validate() {
        let init = CustomProcessBuilder::new()
            .with_name("init")
            .with_command("sh")
            .with_one_shot()
            .build()
            .unwrap();
        let api = CustomProcessBuilder::new()
            .with_name("api")
            .with_command("api")
            .with_named_port("http", 8080)
            .with_ready_check(ReadyCheck::http("http", "/health"))
            .with_timeout(30)
            .with_dependency("init")
            .with_dependency_on("db", DependencyCondition::ServiceStarted)
            .build()
            .unwrap();
        let db = CustomProcessBuilder::new()
            .with_name("db")
            .with_command("postgres")
            .with_named_port("pg", 5432)
            .build()
            .unwrap();

        assert_eq!(
            init.natural_condition(),
            DependencyCondition::ServiceCompletedSuccessfully
        );
        assert_eq!(db.natural_condition(), DependencyCondition::ServiceHealthy);
        assert_eq!(api.ready_check().unwrap().port_name(), "http");
        assert_eq!(api.timeout(), Some(30));
        assert_eq!(
            api.effective_check(),
            Some(ReadyTarget::Http {
                port: "http".into(),
                path: "/health".into()
            })
        );
        assert_eq!(db.effective_check(), Some(ReadyTarget::Tcp("pg".into())));
        assert_eq!(init.effective_check(), None);

        let toml_str = toml::to_string(&api).unwrap();
        assert!(
            toml_str.contains(
                "depends_on = [\"init\", { name = \"db\", condition = \"service_started\" }]"
            ),
            "{toml_str}"
        );
        assert!(toml_str.contains("[ready_check.http]"), "{toml_str}");
        let back: CustomProcess = toml::from_str(&toml_str).unwrap();
        assert_eq!(back, api);

        validate_custom_processes(&[init.clone(), db.clone(), api.clone()]).unwrap();
    }

    #[test]
    fn lifecycle_rules_are_enforced() {
        // ready_check on a port that is not declared, and on a one-shot
        let errors = CustomProcessBuilder::new()
            .with_name("bad")
            .with_command("sh")
            .with_one_shot()
            .with_ready_check(ReadyCheck::tcp("nope"))
            .build()
            .unwrap_err()
            .1;
        let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            text.iter()
                .any(|e| e.contains("port 'nope' is not declared")),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|e| e.contains("one-shot process runs to completion")),
            "{text:?}"
        );

        // zero and absurd durations
        let errors = CustomProcessBuilder::new()
            .with_name("slow")
            .with_command("sh")
            .with_named_port("p", 1)
            .with_ready_check(ReadyCheck::tcp("p").with_interval(0))
            .with_timeout(0)
            .build()
            .unwrap_err()
            .1;
        let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            text.iter()
                .any(|e| e.contains("interval must be at least 1")),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|e| e.contains("timeout: must be at least 1")),
            "{text:?}"
        );
        let errors = CustomProcessBuilder::new()
            .with_name("forever")
            .with_command("sh")
            .with_timeout(u32::MAX)
            .build()
            .unwrap_err()
            .1;
        assert!(
            errors.iter().any(|e| e.to_string().contains("a week")),
            "{errors:?}"
        );

        // self dependency and a duplicate
        let errors = CustomProcessBuilder::new()
            .with_name("me")
            .with_command("sh")
            .with_dependency("me")
            .with_dependency("other")
            .with_dependency("other")
            .build()
            .unwrap_err()
            .1;
        let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        assert!(
            text.iter().any(|e| e.contains("depends on itself")),
            "{text:?}"
        );
        assert!(text.iter().any(|e| e.contains("listed twice")), "{text:?}");

        // across processes: unknown target, condition that does not fit, a cycle
        let mk = |name: &str, deps: Vec<Dependency>, one_shot: bool| {
            let mut b = CustomProcessBuilder::new()
                .with_name(name)
                .with_command("sh");
            for d in deps {
                b = b.with_dependency_on(d.name, d.condition);
            }
            if one_shot {
                b = b.with_one_shot();
            }
            b.build().unwrap()
        };
        let errors = validate_custom_processes(&[
            mk("a", vec!["ghost".into()], false),
            mk(
                "b",
                vec![Dependency {
                    name: "a".into(),
                    condition: Some(DependencyCondition::ServiceHealthy),
                }],
                false,
            ),
            mk(
                "c",
                vec![Dependency {
                    name: "a".into(),
                    condition: Some(DependencyCondition::ServiceCompletedSuccessfully),
                }],
                false,
            ),
            mk("x", vec!["y".into()], false),
            mk("y", vec!["x".into()], false),
        ])
        .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("'ghost' is not a custom process")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("'a' cannot be service_healthy")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("'a' cannot be service_completed_successfully")),
            "{errors:?}"
        );
        assert!(
            errors.iter().any(|e| e.contains("cycle among x, y")),
            "{errors:?}"
        );
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
                .with_named_port(bad, 5001)
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
                .with_named_port(good, 5001)
                .build()
                .unwrap_or_else(|e| panic!("{good:?} should be accepted, got {:?}", e.1));
        }
    }
}
