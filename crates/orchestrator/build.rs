//! Generates `$OUT_DIR/jam_blobs.rs`.
//!
//! With the `jam` feature, it builds the JAM bootstrap service from the `jam-bootstrap-service`
//! crate into a PVM blob and embeds it, so zombienet can build a JAM genesis without any external
//! file. The blob is built with `cargo-jam-build`, which needs the `rust-src` component of the
//! active toolchain; where it can't be built, `ZOMBIE_JAM_BOOTSTRAP_SERVICE_BLOB` names a
//! prebuilt blob to embed instead. Without the feature, nothing is embedded.

use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("`OUT_DIR` is set for build.rs"));

    #[cfg(feature = "jam")]
    jam::generate(&out_dir);

    #[cfg(not(feature = "jam"))]
    std::fs::write(
        out_dir.join("jam_blobs.rs"),
        "// Built without the `jam` feature: no embedded bootstrap service.\n",
    )
    .expect("writing the module cannot fail");
}

#[cfg(feature = "jam")]
mod jam {
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    /// The crate the bootstrap service is built from, pinned in `Cargo.toml`.
    const BOOTSTRAP_SERVICE_CRATE: &str = "jam-bootstrap-service";

    /// Embed this blob instead of building one.
    const BLOB_ENV: &str = "ZOMBIE_JAM_BOOTSTRAP_SERVICE_BLOB";

    /// The profiles polkajam builds its blobs with. A published crate doesn't carry its workspace's
    /// profiles, so they are added back to the copy that is built.
    const PVM_PROFILES: &str = r#"
[profile.release]
panic = "abort"
debug = true
debug-assertions = true
overflow-checks = true
opt-level = 3

[profile.production]
inherits = "release"
lto = true
codegen-units = 1
debug = false
debug-assertions = false
overflow-checks = false
"#;

    /// Build or read the blob, and write the module embedding it.
    pub fn generate(out_dir: &Path) {
        println!("cargo:rerun-if-env-changed={BLOB_ENV}");
        println!("cargo:rerun-if-env-changed=DOCS_RS");

        let (blob, source) = if env::var_os("DOCS_RS").is_some() {
            // docs.rs builds offline and without the PVM toolchain; the blob is never used there.
            (vec![], "none (docs.rs)".to_string())
        } else if let Some(path) = env::var_os(BLOB_ENV) {
            let path = PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            let blob = fs::read(&path)
                .unwrap_or_else(|e| panic!("{BLOB_ENV}: failed to read {}: {e}", path.display()));
            (blob, path.display().to_string())
        } else {
            let (blob, version) = build_bootstrap_service(out_dir);
            (blob, format!("{BOOTSTRAP_SERVICE_CRATE} {version}"))
        };

        let blob_path = out_dir.join("jam-bootstrap-service.jam");
        fs::write(&blob_path, &blob).expect("writing the blob cannot fail");
        let hash = cargo_jam_build::code_hash(&blob);

        let module = format!(
            "/// Where the embedded bootstrap service comes from.\n\
             pub const BOOTSTRAP_SERVICE_SOURCE: &str = {source:?};\n\
             /// The JAM bootstrap service.\n\
             pub const BOOTSTRAP_SERVICE_BLOB: &[u8] = include_bytes!({blob_path:?});\n\
             /// The code hash of [`BOOTSTRAP_SERVICE_BLOB`].\n\
             pub const BOOTSTRAP_SERVICE_HASH: [u8; 32] = {hash:?};\n"
        );
        fs::write(out_dir.join("jam_blobs.rs"), module).expect("writing the module cannot fail");
    }

    /// Build the bootstrap service from the `jam-bootstrap-service` build dependency, returning the
    /// blob and the crate version.
    fn build_bootstrap_service(out_dir: &Path) -> (Vec<u8>, String) {
        // The build dependency makes cargo fetch the crate; its sources are found through the
        // dependency graph.
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set for build.rs"))
            .join("Cargo.toml");
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&manifest)
            // The build dependency is optional, enabled by this feature.
            .features(cargo_metadata::CargoOpt::SomeFeatures(vec!["jam".into()]))
            .exec()
            .unwrap_or_else(|e| panic!("failed to read the dependency graph: {e}"));
        let package = metadata
            .packages
            .iter()
            .find(|p| p.name.as_str() == BOOTSTRAP_SERVICE_CRATE)
            .unwrap_or_else(|| panic!("`{BOOTSTRAP_SERVICE_CRATE}` is a build dependency"));
        let source_dir = package
            .manifest_path
            .parent()
            .expect("a manifest is in a directory")
            .as_std_path();

        // Built from a copy: a published crate is its own workspace, and the registry's copy must
        // stay untouched.
        let crate_dir = out_dir.join(BOOTSTRAP_SERVICE_CRATE);
        let _ = fs::remove_dir_all(&crate_dir);
        copy_dir(source_dir, &crate_dir);
        let crate_manifest = crate_dir.join("Cargo.toml");
        let mut content = fs::read_to_string(&crate_manifest).expect("the crate has a manifest");
        if !content.contains("[profile.") {
            content.push_str(PVM_PROFILES);
        }
        // `OUT_DIR` may be under the workspace building zombienet; the copy is a workspace of its own.
        if !content.contains("[workspace]") {
            content.push_str("\n[workspace]\n");
        }
        fs::write(&crate_manifest, content).expect("writing the manifest cannot fail");

        let workspace = cargo_jam_build::Workspace::open(Some(&crate_manifest))
            .unwrap_or_else(|e| panic!("failed to read `{BOOTSTRAP_SERVICE_CRATE}`: {e:#}"));
        let options = cargo_jam_build::Options {
            // Never the outer build's target directory, which cargo holds locked.
            target_dir: Some(out_dir.join("pvm-target")),
            ..Default::default()
        };
        let artifacts = workspace
            .build(BOOTSTRAP_SERVICE_CRATE, None, &options)
            .unwrap_or_else(|e| {
                panic!(
                    "failed to build the JAM bootstrap service from `{BOOTSTRAP_SERVICE_CRATE}`: \
                     {e:#}\n\nIt needs the `rust-src` component (`rustup component add rust-src`), \
                     or set {BLOB_ENV} to a prebuilt blob."
                )
            });

        (artifacts.blob.bytes, package.version.to_string())
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).expect("creating a directory in OUT_DIR cannot fail");
        for entry in fs::read_dir(from).expect("the crate sources are readable") {
            let entry = entry.expect("the crate sources are readable");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("readable").is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).expect("copying into OUT_DIR cannot fail");
            }
        }
    }
}
