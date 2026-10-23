//! Resolve `db_snapshot` `AssetLocation`s into local cache paths, once per
//! unique location, **before** the parallel spawn fanout.
//!
//! Cache layout is: `{ns_base_dir}/{sha256(loc_str)}.tgz`. The
//! provider's `initialize_db_snapshot` takes this already-resolved file and
//! only has to extract it.
//!
//! The exception is a URL for a provider whose nodes fetch it themselves
//! ([`ProviderCapabilities::fetches_db_snapshot_url`], k8s): it is passed
//! through untouched as [`DbSnapshot::Url`]. Downloading it here would hold
//! the whole snapshot in this host's memory and upload it again to the
//! namespace's fileserver, for every distinct snapshot — gigabytes each for a
//! forked public network — when each pod can fetch it directly.
//!
//! [`ProviderCapabilities::fetches_db_snapshot_url`]: provider::types::ProviderCapabilities

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use configuration::types::AssetLocation;
use provider::{types::DbSnapshot, DynNamespace, ProviderError};
use sha2::Digest;
use support::fs::FileSystem;
use tracing::trace;

use crate::network_spec::node::NodeSpec;

/// Lookup map produced by [`resolve_db_snapshots`].
pub type ResolvedDbSnapshots = HashMap<AssetLocation, DbSnapshot>;

/// Walk every node's `db_snapshot`, deduplicate by `AssetLocation`, and
/// fetch each unique location once into the namespace cache — unless the
/// nodes fetch URLs themselves (see the module docs). Returns a map from the
/// original `AssetLocation` to what the provider is handed for it.
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
    let fetched_in_node = ns.capabilities().fetches_db_snapshot_url;
    let mut resolved: ResolvedDbSnapshots = HashMap::new();

    for loc in nodes.into_iter().filter_map(|n| n.db_snapshot.as_ref()) {
        if resolved.contains_key(loc) {
            continue;
        }
        let snapshot = match resolution(loc, fetched_in_node, &ns_base_dir) {
            Resolution::InNode(url) => {
                trace!("db_snapshot {loc} is fetched by the nodes themselves");
                DbSnapshot::Url(url)
            },
            Resolution::Cache(cache_path) => {
                if !filesystem.exists(&cache_path).await {
                    fetch_into_cache(loc, &cache_path, filesystem).await?;
                } else {
                    trace!("db_snapshot cache hit: {}", cache_path.display());
                }
                DbSnapshot::Local(cache_path)
            },
        };
        resolved.insert(loc.clone(), snapshot);
    }

    Ok(resolved)
}

/// Where one snapshot comes from, decided without touching anything.
#[derive(Debug, PartialEq)]
enum Resolution {
    /// Handed to the provider as is, for the node to fetch.
    InNode(url::Url),
    /// Fetched or copied here, into this cache file.
    Cache(PathBuf),
}

fn resolution(location: &AssetLocation, fetched_in_node: bool, ns_base_dir: &str) -> Resolution {
    match location {
        AssetLocation::Url(url) if fetched_in_node => Resolution::InNode(url.clone()),
        _ => {
            let hashed = hex::encode(sha2::Sha256::digest(location.to_string()));
            Resolution::Cache(PathBuf::from(format!("{ns_base_dir}/{hashed}.tgz")))
        },
    }
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
    for snapshot in resolved.values() {
        // Nothing was cached for a snapshot the nodes fetched themselves.
        let DbSnapshot::Local(cache_path) = snapshot else {
            continue;
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "/tmp/zombie-ns";

    fn url() -> AssetLocation {
        AssetLocation::Url(
            "https://example.com/snaps/rc.tgz?sig=a&b=c"
                .parse()
                .unwrap(),
        )
    }

    #[test]
    fn a_url_is_left_to_nodes_that_fetch_it_themselves() {
        let AssetLocation::Url(expected) = url() else {
            unreachable!()
        };
        assert_eq!(resolution(&url(), true, BASE), Resolution::InNode(expected));
    }

    #[test]
    fn a_url_is_cached_here_for_nodes_that_do_not() {
        let Resolution::Cache(path) = resolution(&url(), false, BASE) else {
            panic!("expected a cache path");
        };
        assert!(path.starts_with(BASE), "{}", path.display());
        assert_eq!(path.extension().unwrap(), "tgz");
    }

    #[test]
    fn a_local_file_is_always_cached_here() {
        let local = AssetLocation::FilePath("/snaps/rc.tgz".into());
        for fetched_in_node in [true, false] {
            assert!(matches!(
                resolution(&local, fetched_in_node, BASE),
                Resolution::Cache(_)
            ));
        }
    }
}
