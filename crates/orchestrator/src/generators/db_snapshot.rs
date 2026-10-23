//! Resolve `db_snapshot` `AssetLocation`s into local cache paths, once per
//! unique location, **before** the parallel spawn fanout.
//!
//! Cache layout is: `{ns_base_dir}/{sha256(loc_str)}.tgz`. The
//! provider's `initialize_db_snapshot` takes this already-resolved file and
//! only has to extract it.
//!
//! The exception is a URL for a provider whose fileserver can fetch it
//! itself ([`ProviderCapabilities::fileserver_fetches_db_snapshot_url`],
//! k8s). The fileserver downloads it once, under the same
//! `sha256(loc_str).tgz` name, and every node curls it from there as it would
//! an uploaded one. Downloading it here instead would hold the whole snapshot
//! in this host's memory and then upload it to that same fileserver — for
//! every distinct snapshot, gigabytes each for a forked public network.
//!
//! [`ProviderCapabilities::fileserver_fetches_db_snapshot_url`]: provider::types::ProviderCapabilities

use std::{collections::HashMap, path::Path};

use configuration::types::AssetLocation;
use provider::{types::DbSnapshot, DynNamespace, ProviderError};
use sha2::Digest;
use support::fs::FileSystem;
use tracing::trace;

use crate::network_spec::node::NodeSpec;

/// Lookup map produced by [`resolve_db_snapshots`].
pub type ResolvedDbSnapshots = HashMap<AssetLocation, DbSnapshot>;

/// Walk every node's `db_snapshot`, deduplicate by `AssetLocation`, and
/// fetch each unique location once: into the namespace cache, or into the
/// namespace's fileserver (see the module docs). Returns a map from the
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
    let fileserver_fetches = ns.capabilities().fileserver_fetches_db_snapshot_url;
    let mut resolved: ResolvedDbSnapshots = HashMap::new();

    for loc in nodes.into_iter().filter_map(|n| n.db_snapshot.as_ref()) {
        if resolved.contains_key(loc) {
            continue;
        }
        let name = cache_name(loc);
        let in_fileserver = match fetched_by_fileserver(loc, fileserver_fetches) {
            Some(url) => ns.fetch_into_fileserver(url, &name).await?,
            None => None,
        };
        let snapshot = match in_fileserver {
            Some(url) => {
                trace!("db_snapshot {name} fetched by the fileserver: {url}");
                DbSnapshot::InFileserver(url)
            },
            None => {
                let cache_path = Path::new(&ns_base_dir).join(&name);
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

/// The name a snapshot is kept under, in the namespace cache or in the
/// fileserver alike.
fn cache_name(location: &AssetLocation) -> String {
    format!(
        "{}.tgz",
        hex::encode(sha2::Sha256::digest(location.to_string()))
    )
}

/// The URL to hand the fileserver, when it fetches `location` itself.
fn fetched_by_fileserver(location: &AssetLocation, fileserver_fetches: bool) -> Option<&url::Url> {
    match location {
        AssetLocation::Url(url) if fileserver_fetches => Some(url),
        _ => None,
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
        // Nothing was cached here for a snapshot the fileserver fetched.
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

    fn url() -> AssetLocation {
        AssetLocation::Url(
            "https://example.com/snaps/rc.tgz?sig=a&b=c"
                .parse()
                .unwrap(),
        )
    }

    #[test]
    fn a_url_is_fetched_by_a_fileserver_that_can() {
        let AssetLocation::Url(expected) = url() else {
            unreachable!()
        };
        assert_eq!(fetched_by_fileserver(&url(), true), Some(&expected));
        assert_eq!(fetched_by_fileserver(&url(), false), None);
    }

    #[test]
    fn a_local_file_is_never_fetched_by_the_fileserver() {
        let local = AssetLocation::FilePath("/snaps/rc.tgz".into());
        assert_eq!(fetched_by_fileserver(&local, true), None);
    }

    #[test]
    fn a_snapshot_keeps_its_cache_name_wherever_it_is_kept() {
        let name = cache_name(&url());
        assert_eq!(name, cache_name(&url()));
        assert_eq!(name.len(), 64 + ".tgz".len());
        assert_ne!(
            name,
            cache_name(&AssetLocation::FilePath("/snaps/rc.tgz".into()))
        );
    }
}
