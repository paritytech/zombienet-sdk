use std::{collections::BTreeMap, env};

use configuration::shared::resources::{ResourceQuantity, Resources};
use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, ContainerPort, EmptyDirVolumeSource, EnvVar,
        EnvVarSource, HTTPGetAction, PodSpec, Probe, ResourceRequirements, SecretKeySelector,
        TCPSocketAction, Toleration, Volume, VolumeMount,
    },
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
};

use crate::{
    constants::NODE_SCRIPTS_DIR,
    shared::helpers::injected_file_name,
    types::{Port, ReadinessProbe, ReadinessTarget, SecretEnvVar, TransferedFile},
};

/// The volume a direct process's injected files are mounted from, one by one,
/// filled by an init container.
const FILES_VOLUME: &str = "files";

/// The image of the init containers: `wget`, `sha256sum` and `chmod` in it.
const SETUP_IMAGE: &str =
    "europe-west3-docker.pkg.dev/parity-zombienet/zombienet-public-images/alpine:latest";

/// An injected file as the pod's init container fetches it: from the
/// network's fileserver by `url`, checked against `hash` (sha256, hex).
pub(super) struct FileToFetch {
    pub file: TransferedFile,
    pub url: String,
    pub hash: String,
}

pub(super) struct PodSpecBuilder;

impl PodSpecBuilder {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build(
        name: &str,
        image: &str,
        resources: Option<&Resources>,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        secret_env: &[SecretEnvVar],
        files: &[FileToFetch],
        ports: &[(String, Port)],
        readiness: Option<&ReadinessProbe>,
        wrapper: bool,
    ) -> PodSpec {
        let tolerations = if let Ok(node_type) = env::var("X_INFRA_NODETYPE") {
            let t = Toleration {
                effect: Some("NoExecute".into()),
                key: Some("nodetype".into()),
                operator: Some("Equal".into()),
                value: Some(node_type),
                ..Default::default()
            };
            Some(vec![t])
        } else {
            None
        };

        let mut init_containers = vec![Self::build_helper_binaries_setup_container()];
        if !files.is_empty() {
            init_containers.push(Self::build_files_setup_container(files));
        }

        PodSpec {
            hostname: Some(name.to_string()),
            init_containers: Some(init_containers),
            containers: vec![Self::build_main_container(
                name, image, resources, program, args, env, secret_env, files, ports, readiness,
                wrapper,
            )],
            volumes: Some(Self::build_volumes(!files.is_empty())),
            tolerations,
            // A direct process that exits is done, by success or by failure: the
            // pod ends with it, and so says what happened, instead of the
            // kubelet restarting it in a loop. A wrapped node keeps its pod up
            // through the wrapper either way.
            restart_policy: (!wrapper).then(|| "Never".to_string()),
            ..Default::default()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_main_container(
        name: &str,
        image: &str,
        resources: Option<&Resources>,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        secret_env: &[SecretEnvVar],
        files: &[FileToFetch],
        ports: &[(String, Port)],
        readiness: Option<&ReadinessProbe>,
        wrapper: bool,
    ) -> Container {
        // With the wrapper, `program` is started on demand through the pipe the
        // wrapper listens on. Without it the container runs `program` directly,
        // as the image would, so an off-the-shelf image needs no `bash`.
        let command = if wrapper {
            [
                vec!["/zombie-wrapper.sh".to_string(), program.to_string()],
                args.to_vec(),
            ]
            .concat()
        } else {
            [vec![program.to_string()], args.to_vec()].concat()
        };

        let container_ports = if ports.is_empty() {
            None
        } else {
            Some(
                ports
                    .iter()
                    .map(|(name, port)| ContainerPort {
                        name: Some(name.clone()),
                        container_port: i32::from(*port),
                        ..Default::default()
                    })
                    .collect(),
            )
        };

        // A wrapped node reports readiness itself (the wrapper is up at once
        // and the node is waited on through its metrics). A direct process is
        // Ready, and so routed to by its Service, once the declared probe
        // passes on the pod address.
        let readiness_probe = match (wrapper, readiness) {
            (false, Some(probe)) => {
                let (tcp_socket, http_get) = match &probe.target {
                    ReadinessTarget::Tcp(port) => (
                        Some(TCPSocketAction {
                            port: IntOrString::Int(i32::from(*port)),
                            ..Default::default()
                        }),
                        None,
                    ),
                    ReadinessTarget::Http { port, path } => (
                        None,
                        Some(HTTPGetAction {
                            port: IntOrString::Int(i32::from(*port)),
                            path: Some(path.clone()),
                            ..Default::default()
                        }),
                    ),
                };
                Some(Probe {
                    tcp_socket,
                    http_get,
                    period_seconds: Some(i32::try_from(probe.period_secs).unwrap_or(i32::MAX)),
                    // the same patience as the probe zombienet runs itself
                    timeout_seconds: Some(5),
                    failure_threshold: Some(3),
                    ..Default::default()
                })
            },
            _ => None,
        };

        let mut extra_mounts = if wrapper {
            vec![VolumeMount {
                name: "zombie-wrapper-volume".to_string(),
                mount_path: "/zombie-wrapper.sh".to_string(),
                sub_path: Some("zombie-wrapper.sh".to_string()),
                ..Default::default()
            }]
        } else {
            vec![]
        };
        // Each file on its own path, wherever that is in the image.
        extra_mounts.extend(files.iter().enumerate().map(|(index, fetch)| VolumeMount {
            name: FILES_VOLUME.to_string(),
            mount_path: fetch.file.remote_path.to_string_lossy().into_owned(),
            sub_path: Some(injected_file_name(index, &fetch.file)),
            read_only: Some(true),
            ..Default::default()
        }));

        Container {
            name: name.to_string(),
            image: Some(image.to_string()),
            image_pull_policy: Some("Always".to_string()),
            command: Some(command),
            ports: container_ports,
            readiness_probe,
            // A literal value may `$(VAR)` a variable listed before it, a
            // value from a Secret is never expanded, so the secrets go first.
            env: Some(
                secret_env
                    .iter()
                    .map(|var| EnvVar {
                        name: var.name.clone(),
                        value: None,
                        value_from: Some(EnvVarSource {
                            secret_key_ref: Some(SecretKeySelector {
                                name: Some(var.secret.clone()),
                                key: var.key.clone(),
                                optional: None,
                            }),
                            ..Default::default()
                        }),
                    })
                    .chain(env.iter().map(|(name, value)| EnvVar {
                        name: name.clone(),
                        value: Some(value.clone()),
                        value_from: None,
                    }))
                    .collect(),
            ),
            volume_mounts: Some(Self::build_volume_mounts(extra_mounts)),
            resources: Self::build_resources_requirements(resources),
            ..Default::default()
        }
    }

    /// Fetches a direct process's injected files from the network's
    /// fileserver into the `files` volume, checked and with their mode, so
    /// they are in place when `program` starts, whatever the image.
    fn build_files_setup_container(files: &[FileToFetch]) -> Container {
        let script = files
            .iter()
            .enumerate()
            .map(|(index, fetch)| {
                let path = format!("/files/{}", injected_file_name(index, &fetch.file));
                format!(
                    "wget -q -O '{path}' '{}' && echo '{}  {path}' | sha256sum -c && chmod {} '{path}'",
                    fetch.url, fetch.hash, fetch.file.mode
                )
            })
            .collect::<Vec<_>>()
            .join(" && ");
        Container {
            name: "files-setup".to_string(),
            image: Some(SETUP_IMAGE.to_string()),
            image_pull_policy: Some("IfNotPresent".to_string()),
            volume_mounts: Some(vec![VolumeMount {
                name: FILES_VOLUME.to_string(),
                mount_path: "/files".to_string(),
                ..Default::default()
            }]),
            command: Some(vec!["sh".to_string(), "-c".to_string(), script]),
            ..Default::default()
        }
    }

    fn build_helper_binaries_setup_container() -> Container {
        Container {
            name: "helper-binaries-setup".to_string(),
            image: Some(SETUP_IMAGE.to_string()),
            image_pull_policy: Some("IfNotPresent".to_string()),
            volume_mounts: Some(Self::build_volume_mounts(vec![VolumeMount {
                name: "helper-binaries-downloader-volume".to_string(),
                mount_path: "/helper-binaries-downloader.sh".to_string(),
                sub_path: Some("helper-binaries-downloader.sh".to_string()),
                ..Default::default()
            }])),
            command: Some(vec![
                "ash".to_string(),
                "/helper-binaries-downloader.sh".to_string(),
            ]),
            ..Default::default()
        }
    }

    fn build_volumes(with_files: bool) -> Vec<Volume> {
        let mut volumes = vec![
            Volume {
                name: "cfg".to_string(),
                ..Default::default()
            },
            Volume {
                name: "data".to_string(),
                ..Default::default()
            },
            Volume {
                name: "relay-data".to_string(),
                ..Default::default()
            },
            Volume {
                name: "scripts".to_string(),
                ..Default::default()
            },
            Volume {
                name: "zombie-wrapper-volume".to_string(),
                config_map: Some(ConfigMapVolumeSource {
                    name: Some("zombie-wrapper".to_string()),
                    default_mode: Some(0o755),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Volume {
                name: "helper-binaries-downloader-volume".to_string(),
                config_map: Some(ConfigMapVolumeSource {
                    name: Some("helper-binaries-downloader".to_string()),
                    default_mode: Some(0o755),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];
        if with_files {
            volumes.push(Volume {
                name: FILES_VOLUME.to_string(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Default::default()
            });
        }
        volumes
    }

    fn build_volume_mounts(non_default_mounts: Vec<VolumeMount>) -> Vec<VolumeMount> {
        [
            vec![
                VolumeMount {
                    name: "cfg".to_string(),
                    mount_path: "/cfg".to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
                VolumeMount {
                    name: "data".to_string(),
                    mount_path: "/data".to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
                VolumeMount {
                    name: "relay-data".to_string(),
                    mount_path: "/relay-data".to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
                VolumeMount {
                    name: "scripts".to_string(),
                    mount_path: NODE_SCRIPTS_DIR.to_string(),
                    read_only: Some(false),
                    ..Default::default()
                },
            ],
            non_default_mounts,
        ]
        .concat()
    }

    fn build_resources_requirements(resources: Option<&Resources>) -> Option<ResourceRequirements> {
        resources.map(|resources| ResourceRequirements {
            limits: Self::build_resources_requirements_quantities(
                resources.limit_cpu(),
                resources.limit_memory(),
            ),
            requests: Self::build_resources_requirements_quantities(
                resources.request_cpu(),
                resources.request_memory(),
            ),
            ..Default::default()
        })
    }

    fn build_resources_requirements_quantities(
        cpu: Option<&ResourceQuantity>,
        memory: Option<&ResourceQuantity>,
    ) -> Option<BTreeMap<String, Quantity>> {
        let mut quantities = BTreeMap::new();

        if let Some(cpu) = cpu {
            quantities.insert("cpu".to_string(), Quantity(cpu.as_str().to_string()));
        }

        if let Some(memory) = memory {
            quantities.insert("memory".to_string(), Quantity(memory.as_str().to_string()));
        }

        if !quantities.is_empty() {
            Some(quantities)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_container(spec: &PodSpec) -> &Container {
        spec.containers
            .iter()
            .find(|c| c.name == "proc")
            .expect("main container is named after the pod")
    }

    #[test]
    fn a_wrapped_node_runs_through_the_wrapper_and_has_no_probe() {
        let spec = PodSpecBuilder::build(
            "proc",
            "img",
            None,
            "polkadot",
            &["--dev".to_string()],
            &[],
            &[],
            &[],
            &[],
            None,
            true,
        );
        assert!(spec.restart_policy.is_none());
        let container = main_container(&spec);
        assert_eq!(
            container.command.as_deref().unwrap(),
            ["/zombie-wrapper.sh", "polkadot", "--dev"]
        );
        assert!(container.ports.is_none());
        assert!(container.readiness_probe.is_none());
        assert!(container
            .volume_mounts
            .as_ref()
            .unwrap()
            .iter()
            .any(|m| m.name == "zombie-wrapper-volume"));
    }

    #[test]
    fn a_direct_process_runs_its_command_exposes_ports_and_probes_as_told() {
        let ports = vec![("api".to_string(), 5001), ("gateway".to_string(), 8080)];
        let spec = PodSpecBuilder::build(
            "proc",
            "img",
            None,
            "ipfs",
            &["daemon".to_string()],
            &[],
            &[],
            &[],
            &ports,
            Some(&ReadinessProbe {
                target: ReadinessTarget::Http {
                    port: 8080,
                    path: "/version".into(),
                },
                period_secs: 3,
            }),
            false,
        );
        assert_eq!(spec.restart_policy.as_deref(), Some("Never"));
        let container = main_container(&spec);
        assert_eq!(container.command.as_deref().unwrap(), ["ipfs", "daemon"]);

        let container_ports = container.ports.as_ref().unwrap();
        assert_eq!(
            container_ports
                .iter()
                .map(|p| (p.name.as_deref().unwrap(), p.container_port))
                .collect::<Vec<_>>(),
            [("api", 5001), ("gateway", 8080)]
        );

        let probe = container.readiness_probe.as_ref().unwrap();
        let http = probe.http_get.as_ref().unwrap();
        assert_eq!(http.port, IntOrString::Int(8080));
        assert_eq!(http.path.as_deref(), Some("/version"));
        assert_eq!(probe.period_seconds, Some(3));
        assert_eq!(probe.timeout_seconds, Some(5));
        assert!(!container
            .volume_mounts
            .as_ref()
            .unwrap()
            .iter()
            .any(|m| m.name == "zombie-wrapper-volume"));
    }

    #[test]
    fn a_direct_process_without_ports_has_no_probe() {
        let spec = PodSpecBuilder::build(
            "proc",
            "img",
            None,
            "job",
            &[],
            &[],
            &[],
            &[],
            &[],
            None,
            false,
        );
        assert_eq!(spec.restart_policy.as_deref(), Some("Never"));
        let container = main_container(&spec);
        assert!(container.readiness_probe.is_none());
        assert!(container.ports.is_none());
    }

    #[test]
    fn a_tcp_probe_targets_the_given_port() {
        let ports = vec![("api".to_string(), 5001)];
        let spec = PodSpecBuilder::build(
            "proc",
            "img",
            None,
            "ipfs",
            &[],
            &[],
            &[],
            &[],
            &ports,
            Some(&ReadinessProbe {
                target: ReadinessTarget::Tcp(5001),
                period_secs: 1,
            }),
            false,
        );
        let probe = main_container(&spec).readiness_probe.as_ref().unwrap();
        assert_eq!(
            probe.tcp_socket.as_ref().unwrap().port,
            IntOrString::Int(5001)
        );
    }

    #[test]
    fn a_direct_process_mounts_its_files_and_reads_env_from_secrets() {
        let files = vec![
            FileToFetch {
                file: TransferedFile::new("./sql/init.sql", "/schema/init.sql"),
                url: "http://fileserver:80/aa__init.sql".into(),
                hash: "aa".into(),
            },
            FileToFetch {
                file: TransferedFile::new("./run.sh", "/opt/app/run me.sh").mode("0755"),
                url: "http://fileserver:80/bb__run.sh".into(),
                hash: "bb".into(),
            },
        ];
        let secret_env = vec![SecretEnvVar {
            name: "TURN_SECRET".into(),
            secret: "turn".into(),
            key: "secret".into(),
        }];
        let spec = PodSpecBuilder::build(
            "proc",
            "img",
            None,
            "api",
            &[],
            &[("RUST_LOG".to_string(), "info".to_string())],
            &secret_env,
            &files,
            &[],
            None,
            false,
        );

        let volume = spec
            .volumes
            .as_ref()
            .unwrap()
            .iter()
            .find(|v| v.name == FILES_VOLUME)
            .unwrap();
        assert!(volume.empty_dir.is_some());
        let setup = spec
            .init_containers
            .as_ref()
            .unwrap()
            .iter()
            .find(|c| c.name == "files-setup")
            .unwrap();
        assert_eq!(
            setup.command.as_ref().unwrap()[2],
            "wget -q -O '/files/0-init.sql' 'http://fileserver:80/aa__init.sql' && echo 'aa  /files/0-init.sql' | sha256sum -c && chmod 0644 '/files/0-init.sql' && \
             wget -q -O '/files/1-run-me.sh' 'http://fileserver:80/bb__run.sh' && echo 'bb  /files/1-run-me.sh' | sha256sum -c && chmod 0755 '/files/1-run-me.sh'"
        );

        let container = main_container(&spec);
        let mounts: Vec<_> = container
            .volume_mounts
            .as_ref()
            .unwrap()
            .iter()
            .filter(|m| m.name == FILES_VOLUME)
            .map(|m| (m.mount_path.as_str(), m.sub_path.as_deref(), m.read_only))
            .collect();
        assert_eq!(
            mounts,
            [
                ("/schema/init.sql", Some("0-init.sql"), Some(true)),
                ("/opt/app/run me.sh", Some("1-run-me.sh"), Some(true)),
            ]
        );

        let env = container.env.as_ref().unwrap();
        assert_eq!(env[1].value.as_deref(), Some("info"));
        let from_secret = env[0]
            .value_from
            .as_ref()
            .unwrap()
            .secret_key_ref
            .as_ref()
            .unwrap();
        assert_eq!(env[0].name, "TURN_SECRET");
        assert_eq!(from_secret.name.as_deref(), Some("turn"));
        assert_eq!(from_secret.key, "secret");
    }
}
