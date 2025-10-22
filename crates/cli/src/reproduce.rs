use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

const NEXTEST_ARCHIVE_EXTENSION: &str = "tar.zst";
const TAR_EXTENSION: &str = "tar";
const ZST_EXTENSION: &str = "zst";

const DEFAULT_ARTIFACT_PATTERN: &str = "*zombienet-artifacts*";

const DOCKER_IMAGE: &str =
    "docker.io/paritytech/ci-unified:bullseye-1.88.0-2025-06-27-v202506301118";
const DOCKER_ARCHIVE_MOUNT_PATH: &str = "/archive.tar.zst";
const DOCKER_WORKSPACE_MOUNT_PATH: &str = "/workspace";
const DOCKER_BINARIES_PATH: &str = "/tmp/binaries";

const ENV_ZOMBIE_PROVIDER: &str = "ZOMBIE_PROVIDER=native";
const ENV_RUST_LOG: &str = "RUST_LOG=debug";
const ENV_POLKADOT_SDK_PATH: &str = "POLKADOT_SDK_PATH";

const TEMP_DIR_PREFIX: &str = "/tmp/zombienet-reproduce-";
const BINARIES_DIR_PREFIX: &str = "/tmp/zombienet-binaries-";

pub struct ReproduceConfig {
    pub repo: Option<String>,
    pub run_id: Option<String>,
    pub archive_file: Option<String>,
    pub artifact_pattern: Option<String>,
    pub test_filter: Option<Vec<String>>,
}

impl ReproduceConfig {
    pub async fn execute(self) -> Result<()> {
        let (archives, _temp_dir, downloader) = match self.archive_file {
            Some(path) => (
                vec![validate_archive_path(&path, NEXTEST_ARCHIVE_EXTENSION)?],
                None,
                None,
            ),
            None => {
                let repo = self
                    .repo
                    .context("repo is required when not using --archive")?;
                let run_id = self
                    .run_id
                    .context("run_id is required when not using --archive")?;

                let downloader =
                    ArtifactDownloader::new(&repo, &run_id, self.artifact_pattern.as_deref());
                let temp_dir = downloader.create_temp_dir()?;
                let binaries_dir = downloader.create_binaries_dir()?;

                downloader.download_artifacts(&temp_dir)?;
                downloader.download_and_extract_binaries(&binaries_dir)?;

                let archives = downloader.extract_archives(&temp_dir)?;
                let archives: Vec<PathBuf> = archives
                    .iter()
                    .map(|a| validate_archive_path(a, NEXTEST_ARCHIVE_EXTENSION))
                    .collect::<Result<_>>()?;

                (archives, Some(temp_dir), Some(downloader))
            },
        };

        // Use the persistent binaries directory if we downloaded them
        let binaries_dir = downloader
            .as_ref()
            .map(|downloader| downloader.get_binaries_dir());

        TestRunner::new(archives, binaries_dir, self.test_filter).run_all()
    }
}

fn validate_archive_path(path: &str, require_extension: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);

    let canonical = p
        .canonicalize()
        .with_context(|| format!("Failed to resolve path: {}", path))?;

    let meta = std::fs::metadata(&canonical)
        .with_context(|| format!("Failed to stat path: {}", canonical.display()))?;

    if !meta.is_file() {
        anyhow::bail!("Path exists but is not a file: {}", canonical.display());
    }

    if !canonical.to_string_lossy().ends_with(require_extension) {
        anyhow::bail!(
            "Archive does not have expected extension `{}`: {}",
            require_extension,
            canonical.display()
        );
    }

    Ok(canonical)
}

struct ArtifactDownloader {
    repo: String,
    run_id: String,
    artifact_pattern: Option<String>,
}

impl ArtifactDownloader {
    fn new(repo: &str, run_id: &str, artifact_pattern: Option<&str>) -> Self {
        Self {
            repo: repo.to_string(),
            run_id: run_id.to_string(),
            artifact_pattern: artifact_pattern.map(|s| s.to_string()),
        }
    }

    fn create_temp_dir(&self) -> Result<std::path::PathBuf> {
        let temp_dir = std::path::PathBuf::from(format!("{}{}", TEMP_DIR_PREFIX, self.run_id));

        // Remove existing temp directory to avoid conflicts with artifacts
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).context("Failed to remove existing temp directory")?;
        }

        fs::create_dir_all(&temp_dir).context("Failed to create temp directory")?;
        Ok(temp_dir)
    }

    fn create_binaries_dir(&self) -> Result<std::path::PathBuf> {
        let binaries_dir =
            std::path::PathBuf::from(format!("{}{}", BINARIES_DIR_PREFIX, self.run_id));

        // Create directory if it doesn't exist, but don't remove if it does
        if !binaries_dir.exists() {
            fs::create_dir_all(&binaries_dir).context("Failed to create binaries directory")?;
            println!("📁 Created binaries directory: {}", binaries_dir.display());
        } else {
            println!(
                "📁 Using existing binaries directory: {}",
                binaries_dir.display()
            );
        }

        Ok(binaries_dir)
    }

    fn get_binaries_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("{}{}", BINARIES_DIR_PREFIX, self.run_id))
    }

    fn download_artifacts(&self, temp_dir: &Path) -> Result<()> {
        let pattern = self
            .artifact_pattern
            .as_ref()
            .map(|f| format!("*{}*", f))
            .unwrap_or_else(|| DEFAULT_ARTIFACT_PATTERN.to_string());

        println!(
            "⬇️  Downloading nextest archive from GitHub run ID {} in repo paritytech/{}...",
            self.run_id, self.repo
        );
        if let Some(filter) = &self.artifact_pattern {
            println!("   Using filter pattern: *{}*", filter);
        }

        let output = Command::new("gh")
            .args([
                "run",
                "download",
                &self.run_id,
                "--repo",
                &format!("paritytech/{}", self.repo),
                "--pattern",
                &pattern,
                "--dir",
            ])
            .arg(temp_dir)
            .output()
            .context("Failed to execute 'gh' command. Install GitHub CLI and run: gh auth login")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to download artifacts.\n{}{}\nCheck GitHub CLI setup and permissions.",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    fn download_and_extract_binaries(&self, binaries_dir: &Path) -> Result<()> {
        println!(
            "⬇️  Downloading binary artifacts from GitHub run ID {}...",
            self.run_id
        );

        let binary_artifacts = vec![
            "build-linux-stable-cumulus",
            "build-linux-stable-ray",
            "build-linux-stable-alexggh",
            "build-test-parachain",
            "build-test-collators",
            "build-malus",
            "build-templates-node",
            "build-linux-substrate",
        ];

        // Create a marker file to track which artifacts have been processed
        let marker_file = binaries_dir.join(".downloaded");
        let mut processed_artifacts: Vec<String> = if marker_file.exists() {
            fs::read_to_string(&marker_file)
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };

        for artifact in binary_artifacts {
            let artifact_name = format!("{}*", artifact);

            // Check if this artifact was already processed
            if processed_artifacts.contains(&artifact.to_string()) {
                println!("  ✓ Skipping {} (already downloaded)", artifact_name);
                continue;
            }

            println!("  Downloading: {}", artifact_name);

            // Download to a temporary subdirectory first
            let temp_download_dir = binaries_dir.join(format!(".download-{}", artifact));
            fs::create_dir_all(&temp_download_dir)?;

            let output = Command::new("gh")
                .args([
                    "run",
                    "download",
                    &self.run_id,
                    "--repo",
                    &format!("paritytech/{}", self.repo),
                    "--pattern",
                    &artifact_name,
                    "--dir",
                ])
                .arg(&temp_download_dir)
                .output()
                .context("Failed to execute 'gh' command")?;

            if !output.status.success() {
                eprintln!(
                    "  Warning: Failed to download {}: {}",
                    artifact_name,
                    String::from_utf8_lossy(&output.stderr)
                );
                // Clean up temp directory on failure
                let _ = fs::remove_dir_all(&temp_download_dir);
                continue;
            }

            // Extract the zip files and move binaries to the main binaries directory
            self.extract_and_move_binaries(&temp_download_dir, binaries_dir)?;

            // Clean up temp directory after successful extraction
            fs::remove_dir_all(&temp_download_dir)?;

            // Mark this artifact as processed
            processed_artifacts.push(artifact.to_string());
        }

        // Update the marker file
        fs::write(&marker_file, processed_artifacts.join("\n"))?;

        println!("✅ Binary artifacts ready in {}", binaries_dir.display());
        Ok(())
    }

    fn extract_and_move_binaries(&self, source_dir: &Path, dest_dir: &Path) -> Result<()> {
        println!(
            "📦 Extracting and moving binaries from {}...",
            source_dir.display()
        );

        for entry in fs::read_dir(source_dir)?.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                // Look for tar files in subdirectories (e.g., build-linux-stable-xxx/)
                for tar_entry in fs::read_dir(&path)?.filter_map(Result::ok) {
                    let tar_path = tar_entry.path();
                    if tar_path.is_file()
                        && tar_path.extension().and_then(|ext| ext.to_str()) == Some("tar")
                    {
                        println!(
                            "  Extracting: {}",
                            tar_path.file_name().unwrap().to_string_lossy()
                        );
                        let output = Command::new("tar")
                            .args(["-xf"])
                            .arg(&tar_path)
                            .arg("-C")
                            .arg(&path)
                            .output()?;

                        if !output.status.success() {
                            eprintln!(
                                "  Warning: Failed to extract {}: {}",
                                tar_path.display(),
                                String::from_utf8_lossy(&output.stderr)
                            );
                        } else {
                            // Move extracted binaries from the artifacts/ subdirectory to the destination directory
                            let artifacts_dir = path.join("artifacts");
                            if artifacts_dir.exists() {
                                move_binaries_from_dir(&artifacts_dir, dest_dir)?;
                            } else {
                                // Fallback: look in the main directory if artifacts/ doesn't exist
                                move_binaries_from_dir(&path, dest_dir)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn extract_archives(&self, dir: &Path) -> Result<Vec<String>> {
        println!("📦 Extracting downloaded artifacts...");

        ArchiveExtractor::new(dir)
            .extract_zip_files()?
            .extract_tar_files()?
            .find_nextest_archives()
            .and_then(|archives| {
                if archives.is_empty() {
                    anyhow::bail!(
                        "Could not find any nextest archives (.tar.zst) after extraction.\n\
                        Verify that run ID {} in paritytech/{} has nextest test archives.",
                        self.run_id,
                        self.repo
                    );
                }
                self.print_found_archives(&archives);
                Ok(archives)
            })
    }

    fn print_found_archives(&self, archives: &[String]) {
        println!("\n✓ Found {} nextest archive(s):", archives.len());
        for (i, archive) in archives.iter().enumerate() {
            let filename = Path::new(archive).file_name().unwrap().to_string_lossy();
            println!("  {}. {}", i + 1, filename);
        }
    }
}
fn move_binaries_from_dir(src_dir: &Path, dest_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(src_dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() {
            // Skip non-binary files (zip files and files with extensions)
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                // Skip files with extensions - binaries typically have no extension
                if !ext.is_empty() {
                    continue;
                }
            }

            let filename = path.file_name().unwrap();
            let dest = dest_dir.join(filename);

            // Skip if already exists in destination
            if dest.exists() {
                println!("  ✓ Binary already exists: {}", filename.to_string_lossy());
                continue;
            }

            // Copy the binary to the destination (don't remove source)
            fs::copy(&path, &dest).with_context(|| {
                format!("Failed to copy {} to {}", path.display(), dest.display())
            })?;

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&dest)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest, perms)?;
            }

            println!("  ✓ Copied: {}", filename.to_string_lossy());
        } else if path.is_dir() {
            // Recursively move from subdirectories
            move_binaries_from_dir(&path, dest_dir)?;
        }
    }
    Ok(())
}

struct ArchiveExtractor {
    dir: std::path::PathBuf,
}

impl ArchiveExtractor {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn extract_zip_files(self) -> Result<Self> {
        for entry in fs::read_dir(&self.dir)?.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("zip") {
                println!(
                    "  Extracting: {}",
                    path.file_name().unwrap().to_string_lossy()
                );
                let output = Command::new("unzip")
                    .args(["-q", "-o"])
                    .arg(&path)
                    .arg("-d")
                    .arg(&self.dir)
                    .output()?;

                if !output.status.success() {
                    eprintln!(
                        "Warning: Failed to extract zip archive: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
        Ok(self)
    }

    fn extract_tar_files(self) -> Result<Self> {
        extract_tar_files_recursive(&self.dir)?;
        Ok(self)
    }

    fn find_nextest_archives(&self) -> Result<Vec<String>> {
        find_archives_recursive(&self.dir)
    }
}

fn extract_tar_files_recursive(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some(TAR_EXTENSION) {
            println!(
                "  Extracting tar: {}",
                path.file_name().unwrap().to_string_lossy()
            );
            let output = Command::new("tar")
                .args(["-xf"])
                .arg(&path)
                .arg("-C")
                .arg(dir)
                .output()?;

            if !output.status.success() {
                eprintln!(
                    "Warning: Failed to extract tar archive: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        } else if path.is_dir() {
            extract_tar_files_recursive(&path)?;
        }
    }
    Ok(())
}

fn find_archives_recursive(dir: &Path) -> Result<Vec<String>> {
    let mut archives = Vec::new();

    for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                if ext == ZST_EXTENSION
                    && path.to_string_lossy().ends_with(NEXTEST_ARCHIVE_EXTENSION)
                {
                    archives.push(path.to_string_lossy().to_string());
                }
            }
        } else if path.is_dir() {
            archives.extend(find_archives_recursive(&path)?);
        }
    }

    Ok(archives)
}

struct TestRunner {
    archives: Vec<PathBuf>,
    binaries_dir: Option<PathBuf>,
    test_filter: Option<Vec<String>>,
}

impl TestRunner {
    fn new(archives: Vec<PathBuf>, binaries_dir: Option<PathBuf>, test_filter: Option<Vec<String>>) -> Self {
        Self {
            archives,
            binaries_dir,
            test_filter,
        }
    }

    fn run_all(self) -> Result<()> {
        let workspace_path = get_workspace_path()?;

        for (i, archive_path) in self.archives.iter().enumerate() {
            self.print_archive_header(i + 1, self.archives.len());
            self.run_single_archive(archive_path, &workspace_path, self.test_filter.as_ref())?;
        }

        Ok(())
    }

    fn print_archive_header(&self, current: usize, total: usize) {
        println!("═══════════════════════════════════════════════════════════════");
        println!("Running archive {}/{}", current, total);
        println!("═══════════════════════════════════════════════════════════════\n");
    }

    fn run_single_archive(&self, archive_path: &Path, workspace_path: &str, test_filter: Option<&Vec<String>>) -> Result<()> {
        let archive_name = archive_path.file_name().unwrap().to_string_lossy();
        println!("🚀 Running tests from: {}", archive_name);

        let inner_cmd = build_nextest_command(self.binaries_dir.as_ref(), test_filter);
        let mut cmd = build_docker_command(
            archive_path,
            workspace_path,
            self.binaries_dir.as_ref(),
            &inner_cmd,
        );

        let status = cmd
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .context(
                "Failed to execute docker command. Make sure Docker is installed and running.",
            )?;

        if !status.success() {
            eprintln!(
                "❌ Tests from {} failed with exit code: {}\n",
                archive_name,
                status.code().unwrap_or(-1)
            );
            anyhow::bail!("Test execution failed");
        } else {
            println!("✅ Tests from {} completed successfully\n", archive_name);
        }

        Ok(())
    }
}

fn get_workspace_path() -> Result<String> {
    std::env::var(ENV_POLKADOT_SDK_PATH).context(
        "POLKADOT_SDK_PATH environment variable is not set. \
        Please set it to the path of the polkadot-sdk workspace.",
    )
}

fn build_nextest_command(binaries_dir: Option<&PathBuf>, test_filter: Option<&Vec<String>>) -> String {
    let path_export = if binaries_dir.is_some() {
        format!("export PATH={}:$PATH && ", DOCKER_BINARIES_PATH)
    } else {
        "export PATH=/workspace/target/release:$PATH && ".to_string()
    };

    let mut cmd = format!(
        "{}\
        echo $PATH && \
        cd {} && \
        cargo nextest run \
        --archive-file {} \
        --workspace-remap {} \
        --retries 0 \
        --no-capture",
        path_export,
        DOCKER_WORKSPACE_MOUNT_PATH,
        DOCKER_ARCHIVE_MOUNT_PATH,
        DOCKER_WORKSPACE_MOUNT_PATH
    );

    // Add test filter args after --
    if let Some(args) = test_filter {
        if !args.is_empty() {
            cmd.push_str(" -- ");
            cmd.push_str(&args.join(" "));
        }
    }

    cmd
}

fn build_docker_command(
    archive_path: &Path,
    workspace_path: &str,
    binaries_dir: Option<&PathBuf>,
    inner_cmd: &str,
) -> Command {
    let mut cmd = Command::new("docker");

    let archive_mount = format!(
        "{}:{}:ro",
        archive_path.display(),
        DOCKER_ARCHIVE_MOUNT_PATH
    );
    let workspace_mount = format!("{}:{}", workspace_path, DOCKER_WORKSPACE_MOUNT_PATH);

    let mut args = vec![
        "run",
        "-it",
        "--rm",
        "-v",
        &archive_mount,
        "-v",
        &workspace_mount,
    ];

    // Add binaries directory mount if it exists
    let binaries_mount;
    if let Some(bin_dir) = binaries_dir {
        binaries_mount = format!("{}:{}", bin_dir.display(), DOCKER_BINARIES_PATH);
        args.extend_from_slice(&["-v", &binaries_mount]);
    }

    args.extend_from_slice(&[
        "-e",
        ENV_ZOMBIE_PROVIDER,
        "-e",
        ENV_RUST_LOG,
        DOCKER_IMAGE,
        "bash",
        "-c",
        inner_cmd,
    ]);

    cmd.args(args);
    cmd
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_validate_archive_path_existing_file() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_archive.tar.zst");
        fs::write(&test_file, b"test content").unwrap();

        let result = validate_archive_path(test_file.to_str().unwrap(), NEXTEST_ARCHIVE_EXTENSION);
        assert!(result.is_ok());
        let canonical = result.unwrap();

        assert_eq!(canonical.extension().and_then(|e| e.to_str()), Some("zst"));
        assert!(canonical
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(NEXTEST_ARCHIVE_EXTENSION));
    }

    #[test]
    fn test_validate_archive_path_wrong_extension() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_archive.txt");
        fs::write(&test_file, b"test content").unwrap();

        let result = validate_archive_path(test_file.to_str().unwrap(), "tar.zst");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expected extension"));
    }

    #[test]
    fn test_validate_archive_path_nonexistent_file() {
        let result = validate_archive_path(
            "/nonexistent/path/archive.tar.zst",
            NEXTEST_ARCHIVE_EXTENSION,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to resolve path"));
    }

    #[test]
    fn test_validate_archive_path_directory() {
        let temp_dir = std::env::temp_dir();
        let result = validate_archive_path(temp_dir.to_str().unwrap(), NEXTEST_ARCHIVE_EXTENSION);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a file"));
    }

    #[test]
    fn test_find_archives_recursive_finds_tar_zst() {
        let temp_dir = std::env::temp_dir().join("test_find_archives");
        fs::create_dir_all(&temp_dir).unwrap();

        // Create test files
        let archive1 = temp_dir.join("test1.tar.zst");
        let archive2 = temp_dir.join("test2.tar.zst");
        let not_archive = temp_dir.join("test.txt");

        fs::write(&archive1, b"test").unwrap();
        fs::write(&archive2, b"test").unwrap();
        fs::write(&not_archive, b"test").unwrap();

        let result = find_archives_recursive(&temp_dir).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|p| p.contains("test1.tar.zst")));
        assert!(result.iter().any(|p| p.contains("test2.tar.zst")));
        assert!(!result.iter().any(|p| p.contains("test.txt")));
    }

    #[test]
    fn test_find_archives_recursive_nested_dirs() {
        let temp_dir = std::env::temp_dir().join("test_find_nested");
        let nested_dir = temp_dir.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        let archive1 = temp_dir.join("root.tar.zst");
        let archive2 = nested_dir.join("nested.tar.zst");

        fs::write(&archive1, b"test").unwrap();
        fs::write(&archive2, b"test").unwrap();

        let result = find_archives_recursive(&temp_dir).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|p| p.contains("root.tar.zst")));
        assert!(result.iter().any(|p| p.contains("nested.tar.zst")));
    }

    #[test]
    fn test_find_archives_recursive_ignores_zst_without_tar() {
        let temp_dir = std::env::temp_dir().join("test_zst_only");
        fs::create_dir_all(&temp_dir).unwrap();

        let valid_archive = temp_dir.join("valid.tar.zst");
        let invalid_archive = temp_dir.join("invalid.zst");

        fs::write(&valid_archive, b"test").unwrap();
        fs::write(&invalid_archive, b"test").unwrap();

        let result = find_archives_recursive(&temp_dir).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.iter().any(|p| p.contains("valid.tar.zst")));
        assert!(!result.iter().any(|p| p.contains("invalid.zst")));
    }
}
