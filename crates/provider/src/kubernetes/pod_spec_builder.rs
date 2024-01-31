use std::collections::BTreeMap;

use configuration::shared::resources::{ResourceQuantity, Resources};
use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, PodSpec, ResourceRequirements, Volume,
        VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
};

pub(super) struct PodSpecBuilder;

impl PodSpecBuilder {
    pub(super) fn build(
        name: &str,
        image: &str,
        resources: Option<&Resources>,
        program: &str,
        args: &Vec<String>,
        env: &Vec<(String, String)>,
    ) -> PodSpec {
        PodSpec {
            hostname: Some(name.to_string()),
            init_containers: Some(vec![Self::build_helper_binaries_setup_container()]),
            containers: vec![Self::build_main_container(
                name, image, resources, program, args, env,
            )],
            volumes: Some(Self::build_volumes()),
            ..Default::default()
        }
    }

    fn build_main_container(
        name: &str,
        image: &str,
        resources: Option<&Resources>,
        program: &str,
        args: &Vec<String>,
        env: &Vec<(String, String)>,
    ) -> Container {
        Container {
            name: name.to_string(),
            image: Some(image.to_string()),
            image_pull_policy: Some("Always".to_string()),
            command: Some(
                [
                    vec!["/zombie-wrapper.sh".to_string(), program.to_string()],
                    args.clone(),
                ]
                .concat(),
            ),
            env: Some(
                env.iter()
                    .map(|(name, value)| EnvVar {
                        name: name.clone(),
                        value: Some(value.clone()),
                        value_from: None,
                    })
                    .collect(),
            ),
            volume_mounts: Some(Self::build_volume_mounts(vec![VolumeMount {
                name: "zombie-wrapper-volume".to_string(),
                mount_path: "/zombie-wrapper.sh".to_string(),
                sub_path: Some("zombie-wrapper.sh".to_string()),
                ..Default::default()
            }])),
            resources: Self::build_resources_requirements(resources),
            ..Default::default()
        }
    }

    fn build_helper_binaries_setup_container() -> Container {
        Container {
            name: "helper-binaries-setup".to_string(),
            image: Some("docker.io/alpine:latest".to_string()),
            image_pull_policy: Some("Always".to_string()),
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
        vec![
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
            ],
            non_default_mounts,
        ]
        .concat()
    }

    fn build_resources_requirements(resources: Option<&Resources>) -> Option<ResourceRequirements> {
        resources.and_then(|resources| {
            Some(ResourceRequirements {
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
