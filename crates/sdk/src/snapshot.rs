//! Bundle per-node DB snapshots into a single uploadable artifact.
//!
//! The per-node tarballs are produced by [`NetworkNode::snapshot_db`]
//! (in the orchestrator crate). This module packs them — plus a JSON
//! `user_data` blob — into a single outer `bundle.tar.gz` with a
//! [`SnapshotManifest`] sidecar so consumers can verify checksums and
//! discover what's inside.
//!
//! Layout inside the bundle:
//! ```text
//! <archive1>.tgz
//! <archive2>.tgz
//! ...
//! manifest.json   // schema: SnapshotManifest
//! ```
//!
//! Consumer side is user-owned: the user untars the bundle and passes the
//! inner `.tgz` paths to `with_db_snapshot(...)` on a `NodeConfigBuilder`.
//!
//! [`NetworkNode::snapshot_db`]: orchestrator::network::node::NetworkNode::snapshot_db

use std::{
    fs::File,
    io::{self, Write},
    marker::PhantomData,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context};
use chrono::Utc;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use orchestrator::shared::types::NodeSnapshot;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;

/// Result of [`BundleBuilder::build`].
#[derive(Debug, Clone)]
pub struct Bundle {
    /// Absolute path to the produced `bundle.tar.gz`.
    pub path: PathBuf,
    /// Hex-encoded SHA-256 of the outer bundle contents.
    pub sha256: String,
    /// Size of the outer bundle in bytes.
    pub size: u64,
}

/// Schema of `manifest.json` inside the bundle. Versioned —
/// [`MANIFEST_SCHEMA_VERSION`] bumps are breaking changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    /// RFC 3339 timestamp at bundle-build time.
    pub created_at: String,
    /// Collection of [`ArchiveEntry`]s
    pub archives: Vec<ArchiveEntry>,
    /// Caller-provided payload from [`BundleBuilder::user_data`]. Free-form;
    /// shape is the test author's responsibility.
    pub user_data: serde_json::Value,
}

/// Per-archive metadata inside a [`SnapshotManifest`].
/// This is similar to [`NodeSnapshot`] but using the _filename_
/// instead of the path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    /// Basename inside the bundle (e.g. `"relaychain-db.tgz"`).
    pub file: String,
    /// Hex-encoded SHA-256 of the archive's bytes.
    pub sha256: String,
    /// Size of the archive in bytes.
    pub size: u64,
    /// Name of the node that produced this archive.
    pub node_name: String,
}

/// Stable schema version of [`SnapshotManifest`]. Bump on breaking changes.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Typestate marker: no archives added yet.
pub struct Empty;
/// Typestate marker: at least one archive has been added.
pub struct NonEmpty;

/// Assembles a single `bundle.tar.gz` from one or more [`NodeSnapshot`]s
/// plus a JSON `user_data` blob.
///
/// The typestate makes [`build`](BundleBuilder::build) unreachable until
/// at least one archive has been added.
///
/// # Example
/// ```ignore
/// let bundle = BundleBuilder::new()
///     .add(relay_snap)
///     .add(para_snap)
///     .user_data(json!({ "snapshot_height": 930 }))
///     .build("bundle.tar.gz")?;
/// ```
pub struct BundleBuilder<S = Empty> {
    snaps: Vec<NodeSnapshot>,
    user_data: serde_json::Value,
    _state: PhantomData<S>,
}

impl Default for BundleBuilder<Empty> {
    fn default() -> Self {
        Self::new()
    }
}

impl BundleBuilder<Empty> {
    pub fn new() -> Self {
        Self {
            snaps: Vec::new(),
            user_data: serde_json::Value::Null,
            _state: PhantomData,
        }
    }

    /// Add the first per-node archive. Transitions the builder to
    /// [`NonEmpty`], which is the only state that exposes
    /// [`build`](BundleBuilder::build).
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, snap: NodeSnapshot) -> BundleBuilder<NonEmpty> {
        self.snaps.push(snap);
        BundleBuilder {
            snaps: self.snaps,
            user_data: self.user_data,
            _state: PhantomData,
        }
    }
}

impl BundleBuilder<NonEmpty> {
    /// Add a subsequent per-node archive.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, snap: NodeSnapshot) -> Self {
        self.snaps.push(snap);
        self
    }

    /// Produce `out_path` (gzipped outer tarball). Only callable once at
    /// least one archive has been added — enforced at compile time.
    pub fn build(self, out_path: impl AsRef<Path>) -> anyhow::Result<Bundle> {
        let out_path = out_path.as_ref().to_path_buf();
        build_bundle(out_path, self.snaps, self.user_data)
    }
}

impl<S> BundleBuilder<S> {
    /// Attach an arbitrary serializable blob. Stored as JSON under
    /// `user_data` in the manifest. Test authors put block heights,
    /// CIDs, release tags, "number of collators", etc. here. Can be
    /// called before or after [`add`](BundleBuilder::add); last call wins.
    pub fn user_data<T: Serialize>(mut self, data: T) -> Self {
        self.user_data = serde_json::to_value(&data).unwrap_or(serde_json::Value::Null);
        self
    }
}

fn build_bundle(
    out_path: PathBuf,
    snaps: Vec<NodeSnapshot>,
    user_data: serde_json::Value,
) -> anyhow::Result<Bundle> {
    // Build the manifest from the per-archive metadata the orchestrator
    // already computed when each .tgz was produced.
    let entries: Vec<ArchiveEntry> = snaps
        .iter()
        .map(|snap| {
            let file = snap
                .path
                .file_name()
                .ok_or_else(|| anyhow!("snapshot path {} has no filename", snap.path.display()))?
                .to_string_lossy()
                .into_owned();
            Ok::<_, anyhow::Error>(ArchiveEntry {
                file,
                sha256: snap.sha256.clone(),
                size: snap.size,
                node_name: snap.node_name.clone(),
            })
        })
        .collect::<Result<_, _>>()?;

    let manifest = SnapshotManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        created_at: Utc::now().to_rfc3339(),
        archives: entries,
        user_data,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).context("serialising SnapshotManifest")?;

    // Tar everything in: per-node .tgzs (read from disk) + manifest.json
    // (in memory). Top-level entries are flat — no subdirectory.
    let f = File::create(&out_path).with_context(|| format!("creating {}", out_path.display()))?;
    let gz = GzEncoder::new(f, Compression::default());
    let mut tar = tar::Builder::new(gz);

    for (snap, entry) in snaps.iter().zip(manifest.archives.iter()) {
        tar.append_path_with_name(&snap.path, &entry.file)
            .with_context(|| format!("appending {} as {}", snap.path.display(), entry.file))?;
    }

    {
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        header.set_cksum();
        tar.append_data(&mut header, "manifest.json", manifest_bytes.as_slice())
            .context("appending manifest.json")?;
    }

    let gz = tar.into_inner().context("finishing tar")?;
    let mut f = gz.finish().context("finishing gzip")?;
    f.flush().context("flushing bundle file")?;
    drop(f);

    let mut file = File::open(&out_path)
        .with_context(|| format!("reading produced bundle {}", out_path.display()))?;
    let mut sha256 = Sha256::new();
    let size = io::copy(&mut file, &mut sha256).with_context(|| {
        format!(
            "can not copy from file {} to generate hash",
            out_path.display()
        )
    })?;
    let sha256 = hex::encode(sha256.finalize());

    Ok(Bundle {
        path: out_path,
        sha256,
        size,
    })
}

/// Helper function to untar the produced bundle into a destiantion path.
pub fn untar_bundle(bundle_path: &Path, out_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let f = File::open(bundle_path)?;
    let gz = GzDecoder::new(f);
    let mut archive = Archive::new(gz);
    archive.unpack(out_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use flate2::read::GzDecoder;
    use serde_json::json;
    use sha2::Digest;
    use tar::Archive;

    use super::*;

    fn sha256_of(bytes: &[u8]) -> String {
        hex::encode(sha2::Sha256::digest(bytes))
    }

    /// Write `bytes` to `dir/name` and return a `NodeSnapshot` describing it,
    /// mirroring what `NetworkNode::snapshot_db` records on disk.
    fn fake_snapshot(dir: &Path, name: &str, node_name: &str, bytes: &[u8]) -> NodeSnapshot {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write dummy archive");
        NodeSnapshot {
            path,
            sha256: sha256_of(bytes),
            size: bytes.len() as u64,
            node_name: node_name.to_string(),
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zombie-bundle-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn unpack(bundle: &Path, into: &Path) {
        std::fs::create_dir_all(into).expect("create extract dir");
        let f = std::fs::File::open(bundle).expect("open bundle");
        Archive::new(GzDecoder::new(f))
            .unpack(into)
            .expect("unpack bundle");
    }

    // NOTE: the typestate guarantee (`build` is only callable after at least
    // one `add`) is enforced at compile time — `BundleBuilder::new().build()`
    // does not compile — so it isn't exercised here.

    #[test]
    fn build_produces_bundle_and_manifest() {
        let dir = temp_dir();
        let relay_bytes = b"dummy-relay-db-contents".as_slice();
        let para_bytes = b"dummy-para-db-contents-longer".as_slice();
        let relay = fake_snapshot(&dir, "relaychain-db.tgz", "alice", relay_bytes);
        let para = fake_snapshot(&dir, "parachain-db.tgz", "collator-1", para_bytes);

        let bundle = BundleBuilder::new()
            .add(relay.clone())
            .add(para.clone())
            .user_data(json!({ "snapshot_height": 42 }))
            .build(dir.join("bundle.tar.gz"))
            .expect("bundle builds");

        // Bundle metadata matches the file on disk.
        assert!(bundle.path.is_file());
        let on_disk = std::fs::read(&bundle.path).expect("read bundle");
        assert_eq!(sha256_of(&on_disk), bundle.sha256);
        assert_eq!(on_disk.len() as u64, bundle.size);

        // Bundle contains exactly the two archives + manifest.json.
        let extracted = dir.join("extracted");
        unpack(&bundle.path, &extracted);
        let entries: BTreeSet<String> = std::fs::read_dir(&extracted)
            .expect("read extract dir")
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries,
            BTreeSet::from([
                "relaychain-db.tgz".to_string(),
                "parachain-db.tgz".to_string(),
                "manifest.json".to_string(),
            ])
        );

        // Inner archive bytes round-trip unchanged.
        assert_eq!(
            std::fs::read(extracted.join("relaychain-db.tgz")).unwrap(),
            relay_bytes
        );
        assert_eq!(
            std::fs::read(extracted.join("parachain-db.tgz")).unwrap(),
            para_bytes
        );

        // Manifest content.
        let manifest: SnapshotManifest =
            serde_json::from_slice(&std::fs::read(extracted.join("manifest.json")).unwrap())
                .expect("manifest deserialises");
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert!(!manifest.created_at.is_empty());
        assert_eq!(manifest.user_data["snapshot_height"], json!(42));
        assert_eq!(manifest.archives.len(), 2);

        for (entry, snap) in manifest.archives.iter().zip([&relay, &para]) {
            assert_eq!(entry.file, snap.path.file_name().unwrap().to_string_lossy());
            assert_eq!(entry.sha256, snap.sha256);
            assert_eq!(entry.size, snap.size);
            assert_eq!(entry.node_name, snap.node_name);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_archive_default_user_data() {
        let dir = temp_dir();
        let snap = fake_snapshot(&dir, "relaychain-db.tgz", "alice", b"x");

        let bundle = BundleBuilder::new()
            .add(snap)
            .build(dir.join("bundle.tar.gz"))
            .expect("bundle builds");

        let extracted = dir.join("extracted");
        unpack(&bundle.path, &extracted);
        let entries: BTreeSet<String> = std::fs::read_dir(&extracted)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries,
            BTreeSet::from(["relaychain-db.tgz".to_string(), "manifest.json".to_string()])
        );

        let manifest: SnapshotManifest =
            serde_json::from_slice(&std::fs::read(extracted.join("manifest.json")).unwrap())
                .expect("manifest deserialises");
        assert_eq!(manifest.archives.len(), 1);
        assert_eq!(manifest.user_data, serde_json::Value::Null);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
