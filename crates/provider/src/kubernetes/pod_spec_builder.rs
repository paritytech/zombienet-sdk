use std::{collections::BTreeMap, env};

use configuration::shared::resources::{ResourceQuantity, Resources};
use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, ContainerPort, EnvVar, PodSpec, Probe,
        ResourceRequirements, TCPSocketAction, Toleration, Volume, VolumeMount,
    },
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
};

use crate::{constants::NODE_SCRIPTS_DIR, types::Port};

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
        ports: &[(String, Port)],
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

        PodSpec {
            hostname: Some(name.to_string()),
            init_containers: Some(vec![Self::build_helper_binaries_setup_container()]),
            containers: vec![Self::build_main_container(
                name, image, resources, program, args, env, ports, wrapper,
            )],
            volumes: Some(Self::build_volumes()),
            tolerations,
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
        ports: &[(String, Port)],
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
        // Ready, and so routed to by its Service, once its first declared
        // port accepts a connection on the pod address.
        let readiness_probe = match (wrapper, ports.first()) {
            (false, Some((_, port))) => Some(Probe {
                tcp_socket: Some(TCPSocketAction {
                    port: IntOrString::Int(i32::from(*port)),
                    ..Default::default()
                }),
                period_seconds: Some(2),
                failure_threshold: Some(3),
                ..Default::default()
            }),
            _ => None,
        };

        let extra_mounts = if wrapper {
            vec![VolumeMount {
                name: "zombie-wrapper-volume".to_string(),
                mount_path: "/zombie-wrapper.sh".to_string(),
                sub_path: Some("zombie-wrapper.sh".to_string()),
                ..Default::default()
            }]
        } else {
            vec![]
        };

        Container {
            name: name.to_string(),
            image: Some(image.to_string()),
            image_pull_policy: Some("Always".to_string()),
            command: Some(command),
            ports: container_ports,
            readiness_probe,
            env: Some(
                env.iter()
                    .map(|(name, value)| EnvVar {
                        name: name.clone(),
                        value: Some(value.clone()),
                        value_from: None,
                    })
                    .collect(),
            ),
            volume_mounts: Some(Self::build_volume_mounts(extra_mounts)),
            resources: Self::build_resources_requirements(resources),
            ..Default::default()
        }
    }

    fn build_helper_binaries_setup_container() -> Container {
        Container {
            name: "helper-binaries-setup".to_string(),
            image: Some("europe-west3-docker.pkg.dev/parity-zombienet/zombienet-public-images/alpine:latest".to_string()),
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

    fn build_volumes() -> Vec<Volume> {
        vec![
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
        ]
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
            true,
        );
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
    fn a_direct_process_runs_its_command_exposes_ports_and_waits_on_the_first() {
        let ports = vec![("api".to_string(), 5001), ("gateway".to_string(), 8080)];
        let spec = PodSpecBuilder::build(
            "proc",
            "img",
            None,
            "ipfs",
            &["daemon".to_string()],
            &[],
            &ports,
            false,
        );
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
        assert_eq!(
            probe.tcp_socket.as_ref().unwrap().port,
            IntOrString::Int(5001)
        );
        assert!(!container
            .volume_mounts
            .as_ref()
            .unwrap()
            .iter()
            .any(|m| m.name == "zombie-wrapper-volume"));
    }

    #[test]
    fn a_direct_process_without_ports_has_no_probe() {
        let spec = PodSpecBuilder::build("proc", "img", None, "job", &[], &[], &[], false);
        let container = main_container(&spec);
        assert!(container.readiness_probe.is_none());
        assert!(container.ports.is_none());
    }
}
