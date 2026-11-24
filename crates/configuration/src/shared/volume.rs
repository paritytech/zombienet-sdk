//! Volumes backed by a kubernetes `PersistentVolumeClaim`, mounted into a
//! node or a custom process (only k8s).
//!
//! A volume is a `mount_path` plus where its storage comes from:
//!
//! - a **template** (no `claim`): a claim provisioned for the pod, `size`
//!   ([`DEFAULT_VOLUME_SIZE`] when absent), on `storage_class` (the cluster's
//!   default when absent), optionally filled from a `VolumeSnapshot` or another
//!   claim. It lives and dies with the pod.
//! - an existing **claim**, by name, in the namespace the network runs in.
//!   zombienet never creates nor deletes it.
//!
//! A volume on `/cfg`, `/data` or `/relay-data` takes the place of the
//! scratch volume zombienet mounts there; any other path is mounted as well.
//!
//! ```toml
//! [[relaychain.nodes.volumes]]
//! mount_path = "/data"
//! size = "100Gi"
//! storage_class = "premium-rwo"
//!
//! [[relaychain.nodes.volumes]]
//! mount_path = "/opt/fixtures"
//! claim = "fixtures"
//! read_only = true
//! ```

use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use super::resources::ResourceQuantity;
use crate::utils::is_false;

/// The size of a template volume that does not give one.
pub const DEFAULT_VOLUME_SIZE: &str = "20Gi";

/// The paths zombienet mounts a scratch volume at, which a volume replaces.
pub const NODE_VOLUME_PATHS: [&str; 3] = ["/cfg", "/data", "/relay-data"];

/// Paths zombienet owns in the container: no volume may go there.
const RESERVED_PATHS: [&str; 3] = [
    "/scripts",
    "/zombie-wrapper.sh",
    "/helper-binaries-downloader.sh",
];

/// How a provisioned claim can be mounted. There is no `ReadWriteMany`: a
/// volume never has more than one writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessMode {
    /// Read-write by a single pod: the one guarantee a chain database needs.
    /// Needs a CSI driver.
    #[default]
    ReadWriteOncePod,
    /// Read-write by the pods of a single k8s node (pods on the same k8s node
    /// can share it!). For drivers without `ReadWriteOncePod`.
    ReadWriteOnce,
    /// Read-only by many pods; only useful filled `from_snapshot`/`from_claim`.
    ReadOnlyMany,
}

impl AccessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccessMode::ReadWriteOncePod => "ReadWriteOncePod",
            AccessMode::ReadWriteOnce => "ReadWriteOnce",
            AccessMode::ReadOnlyMany => "ReadOnlyMany",
        }
    }
}

impl TryFrom<&str> for AccessMode {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "ReadWriteOncePod" => Ok(AccessMode::ReadWriteOncePod),
            "ReadWriteOnce" => Ok(AccessMode::ReadWriteOnce),
            "ReadOnlyMany" => Ok(AccessMode::ReadOnlyMany),
            "ReadWriteMany" => Err(anyhow!(
                "access_mode 'ReadWriteMany' is not supported, a volume has a single writer"
            )),
            _ => Err(anyhow!(
                "access_mode '{value}' must be one of ReadWriteOncePod, ReadWriteOnce, ReadOnlyMany"
            )),
        }
    }
}

/// What a provisioned claim is filled from, in the same namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeDataSource {
    /// A `VolumeSnapshot`, by name.
    Snapshot(String),
    /// A `PersistentVolumeClaim` to clone, by name.
    Claim(String),
}

/// Where a volume's storage comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum VolumeSource {
    /// A claim provisioned for the pod, deleted with it.
    Template {
        size: ResourceQuantity,
        /// The cluster's default class when `None`.
        storage_class: Option<String>,
        access_mode: AccessMode,
        from: Option<VolumeDataSource>,
    },
    /// An existing claim in the namespace, by name.
    Claim { name: String },
}

/// A volume mounted into a node or custom process (only k8s). See the
/// [module documentation](self).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawNodeVolume", into = "RawNodeVolume")]
pub struct NodeVolume {
    mount_path: PathBuf,
    source: VolumeSource,
    read_only: bool,
    sub_path: Option<String>,
}

impl NodeVolume {
    /// Where the volume is mounted in the container.
    pub fn mount_path(&self) -> &Path {
        &self.mount_path
    }

    /// Where the volume's storage comes from.
    pub fn source(&self) -> &VolumeSource {
        &self.source
    }

    /// Whether the container mounts it read-only.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// The directory of the volume mounted, instead of its root.
    pub fn sub_path(&self) -> Option<&str> {
        self.sub_path.as_deref()
    }

    /// The name of the existing claim, for a volume that is one.
    pub fn claim(&self) -> Option<&str> {
        match &self.source {
            VolumeSource::Claim { name } => Some(name),
            VolumeSource::Template { .. } => None,
        }
    }

    fn from_raw(raw: RawNodeVolume) -> Result<Self, Vec<anyhow::Error>> {
        let mut errors = vec![];

        let path = raw.mount_path.display().to_string();
        if let Err(e) = validate_absolute_path(&raw.mount_path) {
            errors.push(anyhow!("mount_path '{path}' {e}"));
        } else if raw.mount_path == Path::new("/") {
            errors.push(anyhow!("mount_path can't be '/'"));
        } else if let Some(reserved) = RESERVED_PATHS
            .iter()
            .find(|r| raw.mount_path.starts_with(r) || Path::new(r).starts_with(&raw.mount_path))
        {
            errors.push(anyhow!(
                "mount_path '{path}' overlaps '{reserved}', which zombienet owns"
            ));
        }
        if raw.read_only && raw.mount_path == Path::new("/cfg") {
            errors.push(anyhow!(
                "mount_path '/cfg' can't be read_only, zombienet writes there"
            ));
        }

        if let Some(sub_path) = &raw.sub_path {
            let as_path = Path::new(sub_path);
            let plain: PathBuf = as_path.components().collect();
            if sub_path.is_empty()
                || as_path.is_absolute()
                || plain.as_os_str() != as_path.as_os_str()
                || as_path.components().any(|c| c == Component::ParentDir)
            {
                errors.push(anyhow!(
                    "sub_path '{sub_path}' must be a relative path, without '.', '..' or '//'"
                ));
            }
        }

        let source = if let Some(name) = raw.claim {
            let template_fields: Vec<&str> = [
                ("size", raw.size.is_some()),
                ("storage_class", raw.storage_class.is_some()),
                ("access_mode", raw.access_mode.is_some()),
                ("from_snapshot", raw.from_snapshot.is_some()),
                ("from_claim", raw.from_claim.is_some()),
            ]
            .into_iter()
            .filter_map(|(field, set)| set.then_some(field))
            .collect();
            if !template_fields.is_empty() {
                errors.push(anyhow!(
                    "claim '{name}' is an existing claim, {} only apply to a provisioned one",
                    template_fields.join(", ")
                ));
            }
            if let Err(e) = validate_k8s_name(&name) {
                errors.push(anyhow!("claim '{name}' {e}"));
            }
            VolumeSource::Claim { name }
        } else {
            let size_str = raw.size.as_deref().unwrap_or(DEFAULT_VOLUME_SIZE);
            let size = match ResourceQuantity::try_from(size_str) {
                Ok(size) => size,
                Err(e) => {
                    errors.push(anyhow!("size {e}"));
                    // only reached with errors, the value is never used
                    ResourceQuantity::try_from(DEFAULT_VOLUME_SIZE)
                        .expect("the default size is a valid quantity")
                },
            };
            if let Some(class) = &raw.storage_class {
                if let Err(e) = validate_k8s_name(class) {
                    errors.push(anyhow!("storage_class '{class}' {e}"));
                }
            }
            let access_mode = match raw.access_mode.as_deref().map(AccessMode::try_from) {
                None => AccessMode::default(),
                Some(Ok(mode)) => mode,
                Some(Err(e)) => {
                    errors.push(e);
                    AccessMode::default()
                },
            };
            let from = match (raw.from_snapshot, raw.from_claim) {
                (Some(_), Some(_)) => {
                    errors.push(anyhow!("from_snapshot and from_claim can't both be set"));
                    None
                },
                (Some(name), None) => Some(VolumeDataSource::Snapshot(name)),
                (None, Some(name)) => Some(VolumeDataSource::Claim(name)),
                (None, None) => None,
            };
            if let Some(VolumeDataSource::Snapshot(name) | VolumeDataSource::Claim(name)) = &from {
                if let Err(e) = validate_k8s_name(name) {
                    errors.push(anyhow!("'{name}' {e}"));
                }
            }
            if access_mode == AccessMode::ReadOnlyMany && from.is_none() {
                errors.push(anyhow!(
                    "access_mode 'ReadOnlyMany' needs from_snapshot or from_claim, an empty read-only volume holds nothing"
                ));
            }
            if access_mode == AccessMode::ReadOnlyMany && !raw.read_only {
                errors.push(anyhow!(
                    "access_mode 'ReadOnlyMany' must be mounted read_only"
                ));
            }
            VolumeSource::Template {
                size,
                storage_class: raw.storage_class,
                access_mode,
                from,
            }
        };

        if errors.is_empty() {
            Ok(NodeVolume {
                mount_path: raw.mount_path,
                source,
                read_only: raw.read_only,
                sub_path: raw.sub_path,
            })
        } else {
            Err(errors)
        }
    }
}

/// The volume as it is written in a config: flat, a `claim` or template
/// fields, defaults left out.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNodeVolume {
    mount_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claim: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from_claim: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sub_path: Option<String>,
}

impl TryFrom<RawNodeVolume> for NodeVolume {
    type Error = String;

    fn try_from(raw: RawNodeVolume) -> Result<Self, Self::Error> {
        let path = raw.mount_path.display().to_string();
        NodeVolume::from_raw(raw).map_err(|errors| {
            format!(
                "volume '{path}': {}",
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }
}

impl From<NodeVolume> for RawNodeVolume {
    fn from(volume: NodeVolume) -> Self {
        let mut raw = RawNodeVolume {
            mount_path: volume.mount_path,
            read_only: volume.read_only,
            sub_path: volume.sub_path,
            ..Default::default()
        };
        match volume.source {
            VolumeSource::Claim { name } => raw.claim = Some(name),
            VolumeSource::Template {
                size,
                storage_class,
                access_mode,
                from,
            } => {
                raw.size = (size.as_str() != DEFAULT_VOLUME_SIZE).then(|| size.as_str().into());
                raw.storage_class = storage_class;
                raw.access_mode =
                    (access_mode != AccessMode::default()).then(|| access_mode.as_str().into());
                match from {
                    Some(VolumeDataSource::Snapshot(name)) => raw.from_snapshot = Some(name),
                    Some(VolumeDataSource::Claim(name)) => raw.from_claim = Some(name),
                    None => {},
                }
            },
        }
        raw
    }
}

/// A volume builder, used to build a [`NodeVolume`] declaratively with fields
/// validation. Without [`Self::with_claim`] the volume is provisioned for the
/// pod, [`DEFAULT_VOLUME_SIZE`] unless [`Self::with_size`] says otherwise.
#[derive(Debug, Default)]
pub struct NodeVolumeBuilder {
    raw: RawNodeVolume,
}

impl NodeVolumeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the volume is mounted in the container.
    pub fn with_mount_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.raw.mount_path = path.into();
        self
    }

    /// Mount the existing claim `name` (in the network's namespace).
    pub fn with_claim(mut self, name: impl Into<String>) -> Self {
        self.raw.claim = Some(name.into());
        self
    }

    /// Size of the provisioned claim, a k8s quantity (e.g. `100Gi`).
    pub fn with_size(mut self, size: impl Into<String>) -> Self {
        self.raw.size = Some(size.into());
        self
    }

    /// Storage class of the provisioned claim.
    pub fn with_storage_class(mut self, class: impl Into<String>) -> Self {
        self.raw.storage_class = Some(class.into());
        self
    }

    /// Access mode of the provisioned claim, [`AccessMode::ReadWriteOncePod`]
    /// by default.
    pub fn with_access_mode(mut self, mode: AccessMode) -> Self {
        self.raw.access_mode = Some(mode.as_str().into());
        self
    }

    /// Fill the provisioned claim from the `VolumeSnapshot` `name`.
    pub fn with_from_snapshot(mut self, name: impl Into<String>) -> Self {
        self.raw.from_snapshot = Some(name.into());
        self
    }

    /// Fill the provisioned claim as a clone of the claim `name`.
    pub fn with_from_claim(mut self, name: impl Into<String>) -> Self {
        self.raw.from_claim = Some(name.into());
        self
    }

    /// Mount the volume read-only.
    pub fn with_read_only(mut self) -> Self {
        self.raw.read_only = true;
        self
    }

    /// Mount the directory `sub_path` of the volume instead of its root.
    pub fn with_sub_path(mut self, sub_path: impl Into<String>) -> Self {
        self.raw.sub_path = Some(sub_path.into());
        self
    }

    pub fn build(self) -> Result<NodeVolume, Vec<anyhow::Error>> {
        NodeVolume::from_raw(self.raw)
    }
}

/// The rules between the volumes of one node or custom process.
pub fn validate_volumes(volumes: &[NodeVolume]) -> Result<(), Vec<anyhow::Error>> {
    let mut errors = vec![];
    let mut paths = HashSet::new();
    let mut claims = HashSet::new();
    for volume in volumes {
        if !paths.insert(volume.mount_path()) {
            errors.push(anyhow!(
                "mount_path '{}' is given twice",
                volume.mount_path().display()
            ));
        }
        if let Some(claim) = volume.claim() {
            if !claims.insert(claim) {
                errors.push(anyhow!("claim '{claim}' is mounted twice"));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The rules of a chain's `default_volumes`: those of [`validate_volumes`],
/// and no existing claim, which every node of the chain would mount.
pub fn validate_default_volumes(volumes: &[NodeVolume]) -> Result<(), Vec<anyhow::Error>> {
    let mut errors = validate_volumes(volumes).err().unwrap_or_default();
    for volume in volumes {
        if let Some(claim) = volume.claim() {
            errors.push(anyhow!(
                "'{}': claim '{claim}' would be mounted by every node, a default can only be provisioned",
                volume.mount_path().display()
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// A node's volumes over its chain's defaults: the node's own, plus each
/// default on a path the node does not mount.
pub fn merge_volumes(defaults: &[NodeVolume], own: &[NodeVolume]) -> Vec<NodeVolume> {
    defaults
        .iter()
        .filter(|default| !own.iter().any(|v| v.mount_path == default.mount_path))
        .chain(own)
        .cloned()
        .collect()
}

fn validate_absolute_path(path: &Path) -> Result<(), anyhow::Error> {
    let plain: PathBuf = path.components().collect();
    if !path.is_absolute()
        || path.to_string_lossy().contains(':')
        || plain.as_os_str() != path.as_os_str()
        || path.components().any(|c| c == Component::ParentDir)
    {
        return Err(anyhow!(
            "must be an absolute path, without '.', '..', '//', ':' or a trailing '/'"
        ));
    }
    Ok(())
}

/// A DNS-1123 subdomain, what k8s names a claim, a class or a snapshot by.
fn validate_k8s_name(name: &str) -> Result<(), anyhow::Error> {
    let valid_chars = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    let alnum_ends = name
        .chars()
        .next()
        .zip(name.chars().last())
        .is_some_and(|(first, last)| first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric());
    if name.is_empty() || name.len() > 253 || !valid_chars || !alnum_ends {
        return Err(anyhow!(
            "must be lowercase letters, digits, '-' or '.', starting and ending with a letter or digit, at most 253 characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(path: &str) -> NodeVolume {
        NodeVolumeBuilder::new()
            .with_mount_path(path)
            .build()
            .unwrap()
    }

    fn errors_of(builder: NodeVolumeBuilder) -> String {
        builder
            .build()
            .unwrap_err()
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_volume_without_claim_is_a_20gi_template() {
        let volume = template("/data");
        assert_eq!(
            volume.source(),
            &VolumeSource::Template {
                size: ResourceQuantity::try_from("20Gi").unwrap(),
                storage_class: None,
                access_mode: AccessMode::ReadWriteOncePod,
                from: None,
            }
        );
        assert!(!volume.read_only());
        assert_eq!(volume.claim(), None);
    }

    #[test]
    fn toml_round_trip_leaves_defaults_out() {
        let toml_in = r#"
[[volumes]]
mount_path = "/data"

[[volumes]]
mount_path = "/relay-data"
size = "50Gi"
storage_class = "premium-rwo"
access_mode = "ReadWriteOnce"
from_snapshot = "relay-db"

[[volumes]]
mount_path = "/opt/fixtures"
claim = "fixtures"
read_only = true
sub_path = "alice"
"#;
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            volumes: Vec<NodeVolume>,
        }
        let wrapper: Wrapper = toml::from_str(toml_in).unwrap();
        assert_eq!(wrapper.volumes.len(), 3);
        assert_eq!(wrapper.volumes[2].claim(), Some("fixtures"));
        assert_eq!(wrapper.volumes[2].sub_path(), Some("alice"));
        assert!(matches!(
            wrapper.volumes[1].source(),
            VolumeSource::Template {
                access_mode: AccessMode::ReadWriteOnce,
                from: Some(VolumeDataSource::Snapshot(name)),
                ..
            } if name == "relay-db"
        ));

        let dumped = toml::to_string(&wrapper).unwrap();
        assert_eq!(dumped.trim(), toml_in.trim());
    }

    #[test]
    fn a_claim_takes_no_template_field() {
        let errors = errors_of(
            NodeVolumeBuilder::new()
                .with_mount_path("/data")
                .with_claim("db")
                .with_size("1Gi")
                .with_storage_class("ssd"),
        );
        assert!(
            errors.contains("size, storage_class only apply"),
            "{errors}"
        );
    }

    #[test]
    fn read_write_many_is_refused() {
        let raw = r#"
mount_path = "/data"
access_mode = "ReadWriteMany"
"#;
        let err = toml::from_str::<NodeVolume>(raw).unwrap_err().to_string();
        assert!(err.contains("'ReadWriteMany' is not supported"), "{err}");
    }

    #[test]
    fn unknown_fields_are_refused() {
        let err = toml::from_str::<NodeVolume>("mount_path = \"/data\"\nsizee = \"1Gi\"")
            .unwrap_err()
            .to_string();
        assert!(err.contains("sizee"), "{err}");
    }

    #[test]
    fn bad_paths_names_and_sizes_are_refused() {
        for (builder, expected) in [
            (NodeVolumeBuilder::new().with_mount_path("data"), "absolute"),
            (
                NodeVolumeBuilder::new().with_mount_path("/data/"),
                "absolute",
            ),
            (
                NodeVolumeBuilder::new().with_mount_path("/a/../b"),
                "absolute",
            ),
            (
                NodeVolumeBuilder::new().with_mount_path("/"),
                "can't be '/'",
            ),
            (
                NodeVolumeBuilder::new().with_mount_path("/scripts/x"),
                "zombienet owns",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/cfg")
                    .with_read_only(),
                "can't be read_only",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/data")
                    .with_claim("Bad_Name"),
                "claim 'Bad_Name' must be",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/data")
                    .with_size("20Gb"),
                "size",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/data")
                    .with_sub_path("../x"),
                "sub_path",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/data")
                    .with_from_snapshot("a")
                    .with_from_claim("b"),
                "can't both be set",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/data")
                    .with_access_mode(AccessMode::ReadOnlyMany)
                    .with_read_only(),
                "needs from_snapshot or from_claim",
            ),
            (
                NodeVolumeBuilder::new()
                    .with_mount_path("/data")
                    .with_access_mode(AccessMode::ReadOnlyMany)
                    .with_from_snapshot("snap"),
                "must be mounted read_only",
            ),
        ] {
            let errors = errors_of(builder);
            assert!(errors.contains(expected), "{expected}: {errors}");
        }
    }

    #[test]
    fn a_node_mounts_a_path_and_a_claim_once() {
        let claim = |path: &str| {
            NodeVolumeBuilder::new()
                .with_mount_path(path)
                .with_claim("db")
                .build()
                .unwrap()
        };
        let errors = validate_volumes(&[template("/data"), template("/data")]).unwrap_err();
        assert!(errors[0].to_string().contains("given twice"));
        let errors = validate_volumes(&[claim("/a"), claim("/b")]).unwrap_err();
        assert!(errors[0].to_string().contains("mounted twice"));
        validate_volumes(&[template("/data"), claim("/b")]).unwrap();
    }

    #[test]
    fn defaults_are_only_templates() {
        let claim = NodeVolumeBuilder::new()
            .with_mount_path("/data")
            .with_claim("db")
            .build()
            .unwrap();
        let errors = validate_default_volumes(&[claim]).unwrap_err();
        assert!(errors[0].to_string().contains("every node"));
        validate_default_volumes(&[template("/data")]).unwrap();
    }

    #[test]
    fn a_node_volume_replaces_the_default_on_its_path() {
        let big = NodeVolumeBuilder::new()
            .with_mount_path("/data")
            .with_size("100Gi")
            .build()
            .unwrap();
        let merged = merge_volumes(
            &[template("/data"), template("/relay-data")],
            std::slice::from_ref(&big),
        );
        assert_eq!(merged, vec![template("/relay-data"), big]);
    }
}
