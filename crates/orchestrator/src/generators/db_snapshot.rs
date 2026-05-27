//! Resolve `db_snapshot` `AssetLocation`s into local cache paths, once per
//! unique location, **before** the parallel spawn fanout.
//!
//! Cache layout is: `{ns_base_dir}/{sha256(loc_str)}.tgz`. The
//! provider's `initialize_db_snapshot` now takes a `&Path` to this
//! already-resolved file and only has to extract it.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use configuration::types::AssetLocation;
use provider::{DynNamespace, ProviderError};
use sha2::Digest;
use support::fs::FileSystem;
use tracing::trace;

use crate::network_spec::node::NodeSpec;

/// Lookup map produced by [`resolve_db_snapshots`].
pub type ResolvedDbSnapshots = HashMap<AssetLocation, PathBuf>;

/// Walk every node's `db_snapshot`, deduplicate by `AssetLocation`, and
/// fetch each unique location once into the namespace cache. Returns a
/// map from the original `AssetLocation` to the local cache `PathBuf`.
pub async fn resolve_db_snapshots<'a, FS, I>(
    nodes: I,
    ns: &DynNamespace,
    filesystem: &FS,
) -> Result<ResolvedDbSnapshots, ProviderError>
where
    FS: FileSystem,
    I: IntoIterator<Item = &'a NodeSpec>,
{
    let ns_base_dir = ns.base_dir().to_string_lossy().to_string();
    let mut resolved: ResolvedDbSnapshots = HashMap::new();

    for loc in nodes.into_iter().filter_map(|n| n.db_snapshot.as_ref()) {
        if resolved.contains_key(loc) {
            continue;
        }
        let hashed = hex::encode(sha2::Sha256::digest(loc.to_string()));
        let cache_path = PathBuf::from(format!("{ns_base_dir}/{hashed}.tgz"));
        if !filesystem.exists(&cache_path).await {
            fetch_into_cache(loc, &cache_path, filesystem).await?;
        } else {
            trace!("db_snapshot cache hit: {}", cache_path.display());
        }
        resolved.insert(loc.clone(), cache_path);
    }

    Ok(resolved)
}

/// Remove cache tarballs after all consumers have finished extracting them,
/// gated by the `ZOMBIE_RM_TGZ_AFTER_EXTRACT` env var. Best-effort: errors
/// are logged and swallowed (cleanup must never fail spawn).
///
/// Must be called only after every node that consumes the cache has
/// completed its `initialize_db_snapshot` — otherwise concurrent readers
/// will hit `ENOENT`.
pub async fn cleanup_db_snapshot_cache(resolved: &ResolvedDbSnapshots) {
    if std::env::var("ZOMBIE_RM_TGZ_AFTER_EXTRACT").is_err() {
        return;
    }
    for cache_path in resolved.values() {
        match tokio::fs::remove_file(cache_path).await {
            Ok(()) => trace!("removed cache {}", cache_path.display()),
            Err(err) => trace!("failed to remove cache {}: {err}", cache_path.display()),
        }
    }
}

async fn fetch_into_cache<FS: FileSystem>(
    location: &AssetLocation,
    cache_path: &Path,
    filesystem: &FS,
) -> Result<(), ProviderError> {
    trace!(
        "resolving db_snapshot {} -> {}",
        location,
        cache_path.display()
    );
    match location {
        AssetLocation::Url(url) => {
            let res = reqwest::get(url.as_ref())
                .await
                .map_err(|err| ProviderError::DownloadFile(url.to_string(), err.into()))?;
            let bytes = res
                .bytes()
                .await
                .map_err(|err| ProviderError::DownloadFile(url.to_string(), err.into()))?;
            filesystem.write(cache_path, &bytes[..]).await?;
        },
        AssetLocation::FilePath(filepath) => {
            filesystem.copy(filepath, cache_path).await?;
        },
    }
    Ok(())
}
