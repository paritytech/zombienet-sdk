use std::{fs, path::PathBuf};

pub(super) fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub(super) struct TempZombieJson {
    path: PathBuf,
}

impl TempZombieJson {
    pub(super) fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempZombieJson {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn temp_zombie_json(name: &str, contents: &str) -> TempZombieJson {
    let path = unique_temp_path(&format!("zombie-mcp-{name}"), "json");
    fs::write(&path, contents).expect("fixture can be written");
    TempZombieJson { path }
}

/// Generate a unique path inside the OS temp dir for the current test process+thread.
pub(super) fn unique_temp_path(prefix: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{:?}.{extension}",
        std::process::id(),
        std::thread::current().id(),
    ))
}
