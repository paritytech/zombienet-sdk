use serde::Deserializer;
use support::fs::FileSystem;

use crate::{errors::OrchestratorError, ScopedFilesystem};

pub fn default_as_empty_vec<'de, D, T>(_deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Vec::new())
}

pub(crate) async fn write_zombie_json<FS>(
    network: serde_json::Value,
    scoped_fs: ScopedFilesystem<'_, FS>,
    ns_name: &str,
) -> Result<(), OrchestratorError>
where
    FS: FileSystem,
{
    let mut zombie_json = network;

    let base_dir = scoped_fs.base_dir();
    zombie_json["local_base_dir"] = serde_json::value::Value::String(base_dir.to_string());
    zombie_json["ns"] = serde_json::value::Value::String(ns_name.to_string());

    scoped_fs
        .write("zombie.json", serde_json::to_string_pretty(&zombie_json)?)
        .await?;
    Ok(())
}
