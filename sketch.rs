// crates/orchestrator/src/network/node.rs

impl NetworkNode {
    /// Tar this node's database into `out_path` (gzip). Archive layout:
    /// `data/` at the top, plus `relay-data/` for cumulus collators
    /// (auto-detected from the node's context). Directly consumable by
    /// `with_db_snapshot` on a sibling node.
    ///
    /// The caller is responsible for pausing the node before calling this;
    /// snapshotting a running node risks a torn RocksDB state.
    ///
    /// `kind` controls whether per-node identity (`keystore/`, `network/`)
    /// is included — see [`SnapshotKind`].
    pub async fn snapshot_db(
        &self,
        out_path: impl AsRef<Path>,
        kind: SnapshotKind,
    ) -> Result<NodeSnapshot, anyhow::Error>;
}

/// Identity-handling policy for [`NetworkNode::snapshot_db`].
#[derive(Default)]
pub enum SnapshotKind {
    /// Strips the node's identity, only DB. Safe to load on any number of sibling
    /// nodes.
    #[default]
    Shareable,
    /// Includes the node's identity (`keystore/` and `network/`). Use
    /// only when the snapshot will be loaded back into a node that plays
    /// the same role as the source. Loading a `Full` snapshot on a
    /// different node causes equivocation and peer-id collisions.
    Full,
}

/// Result of [`NetworkNode::snapshot_db`].
pub struct NodeSnapshot {
    /// Absolute path to the produced `.tgz`.
    pub path: PathBuf,
    /// Hex-encoded SHA-256 of the archive contents.
    pub sha256: String,
    /// Size of the archive in bytes.
    pub size: u64,
    /// Name of the node this snapshot was taken from.
    pub node_name: String,
}


// crates/orchestrator/src/network.rs

impl<FS> Network<FS> {
    /// Pause every node in the network (SIGSTOP). Issued in parallel.
    pub async fn pause(&self) -> Result<(), anyhow::Error>;

    /// Resume every node in the network (SIGCONT). Issued in parallel.
    pub async fn resume(&self) -> Result<(), anyhow::Error>;
}


// crates/sdk/src/snapshot.rs  (new module, re-exported as zombienet_sdk::snapshot)

/// Assembles a single `bundle.tar.gz` from one or more [`NodeSnapshot`]s
/// plus a JSON `user_data` blob. The bundle is the unit of upload.
///
/// Layout inside the bundle:
/// ```text
/// <archive1>.tgz
/// <archive2>.tgz
/// ...
/// manifest.json   // schema: SnapshotManifest
/// ```
///
/// Uses a typestate to make [`build`](BundleBuilder::build) unreachable
/// until at least one archive has been added. Matches the SDK's
/// `NetworkConfigBuilder<Initial, …>` convention.
pub struct BundleBuilder<S = Empty> { /* … */ }

/// Typestate marker — no archives added yet.
pub struct Empty;
/// Typestate marker — at least one archive has been added.
pub struct NonEmpty;

impl BundleBuilder<Empty> {
    pub fn new() -> Self;

    /// Add the first per-node archive. Transitions the builder to the
    /// [`NonEmpty`] state, which is the only state that exposes
    /// [`build`](BundleBuilder::build).
    pub fn add(self, snap: NodeSnapshot) -> BundleBuilder<NonEmpty>;
}

impl BundleBuilder<NonEmpty> {
    /// Add a subsequent per-node archive.
    pub fn add(self, snap: NodeSnapshot) -> Self;

    /// Produce `out_path` (gzipped tarball). Only callable once at least
    /// one archive has been added — enforced at compile time via the
    /// [`NonEmpty`] typestate.
    pub fn build(self, out_path: impl AsRef<Path>) -> Result<Bundle, anyhow::Error>;
}

impl<S> BundleBuilder<S> {
    /// Attach an arbitrary serializable blob. Stored as JSON under
    /// `user_data` in the manifest. Test authors put block heights,
    /// CIDs, release tags, "number of collators", etc. here. Can be
    /// called before or after [`add`](BundleBuilder::add); last call wins.
    pub fn user_data<T: serde::Serialize>(self, data: T) -> Self;
}

/// Result of [`BundleBuilder::build`].
pub struct Bundle {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
}

/// Schema of `manifest.json` inside the bundle. Versioned —
/// `schema_version` bumps are breaking changes.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    /// RFC 3339 timestamp at bundle-build time.
    pub created_at: String,
    pub archives: Vec<ArchiveEntry>,
    /// Caller-provided payload from [`BundleBuilder::user_data`].
    pub user_data: serde_json::Value,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ArchiveEntry {
    /// Basename inside the bundle (e.g. `"relaychain-db.tgz"`).
    pub file: String,
    pub sha256: String,
    pub size: u64,
    pub node_name: String,
}
