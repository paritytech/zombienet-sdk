use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result};

pub struct ReproduceConfig {
    pub repo: Option<String>,
    pub run_id: Option<String>,
    pub archive_file: Option<String>,
    pub test_filter: Option<String>,
}

impl ReproduceConfig {
    pub async fn execute(self) -> Result<()> {
        let archives = match self.archive_file {
            Some(path) => vec![validate_archive(&path)?],
            None => {
                let repo = self
                    .repo
                    .context("repo is required when not using --archive")?;
                let run_id = self
                    .run_id
                    .context("run_id is required when not using --archive")?;
                ArtifactDownloader::new(&repo, &run_id, self.test_filter.as_deref())
                    .download_and_extract()?
            },
        };

        TestRunner::new(archives).run_all()
    }
}

fn validate_archive(path: &str) -> Result<String> {
    if !fs::metadata(path)
        .context("Failed to access archive file")?
        .is_file()
    {
        anyhow::bail!("Archive file does not exist: {}", path);
    }
    Ok(path.to_string())
}

struct ArtifactDownloader {
    repo: String,
    run_id: String,
    test_filter: Option<String>,
}

impl ArtifactDownloader {
    fn new(repo: &str, run_id: &str, test_filter: Option<&str>) -> Self {
        Self {
            repo: repo.to_string(),
            run_id: run_id.to_string(),
            test_filter: test_filter.map(|s| s.to_string()),
        }
    }

    fn download_and_extract(&self) -> Result<Vec<String>> {
        let temp_dir = self.create_temp_dir()?;
        self.download_artifacts(&temp_dir)?;
        self.extract_archives(&temp_dir)
    }

    fn create_temp_dir(&self) -> Result<std::path::PathBuf> {
        // TODO: Maybe switch to temp dir?
        let temp_dir =
            std::path::PathBuf::from(format!("/tmp/zombienet-reproduce-{}", self.run_id));

        // Remove existing temp directory to avoid conflicts
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).context("Failed to remove existing temp directory")?;
        }

        fs::create_dir_all(&temp_dir).context("Failed to create temp directory")?;
        Ok(temp_dir)
    }

    fn download_artifacts(&self, temp_dir: &Path) -> Result<()> {
        let pattern = self
            .test_filter
            .as_ref()
            .map(|f| format!("*{}*", f))
            .unwrap_or_else(|| "*zombienet-artifacts*".to_string());

        println!(
            "⬇️  Downloading nextest archive from GitHub run ID {} in repo paritytech/{}...",
            self.run_id, self.repo
        );
        if let Some(filter) = &self.test_filter {
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
        println!();
    }
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
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("tar") {
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
                if ext == "zst" && path.to_string_lossy().ends_with(".tar.zst") {
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
    archives: Vec<String>,
}

impl TestRunner {
    fn new(archives: Vec<String>) -> Self {
        Self { archives }
    }

    fn run_all(self) -> Result<()> {
        let workspace_path = get_workspace_path()?;

        for (i, archive_path) in self.archives.iter().enumerate() {
            self.print_archive_header(i + 1, self.archives.len());
            self.run_single_archive(archive_path, &workspace_path)?;
        }

        Ok(())
    }

    fn print_archive_header(&self, current: usize, total: usize) {
        println!("═══════════════════════════════════════════════════════════════");
        println!("Running archive {}/{}", current, total);
        println!("═══════════════════════════════════════════════════════════════\n");
    }

    fn run_single_archive(&self, archive_path: &str, workspace_path: &str) -> Result<()> {
        let archive_abs_path = std::path::PathBuf::from(archive_path)
            .canonicalize()
            .context("Failed to resolve archive path")?;

        let archive_name = archive_abs_path.file_name().unwrap().to_string_lossy();
        println!("🚀 Running tests from: {}", archive_name);

        let inner_cmd = build_nextest_command();
        let mut cmd = build_docker_command(&archive_abs_path, workspace_path, &inner_cmd);

        let status = cmd
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .context(
                "Failed to execute docker command. Make sure Docker is installed and running.",
            )?;

        if !status.success() {
            eprintln!(
                "\n❌ Tests from {} failed with exit code: {:?}\n",
                archive_name,
                status.code()
            );
        } else {
            println!("\n✅ Tests from {} completed successfully\n", archive_name);
        }

        Ok(())
    }
}

fn get_workspace_path() -> Result<String> {
    std::env::var("POLKADOT_SDK_PATH").context(
        "POLKADOT_SDK_PATH environment variable is not set. \
        Please set it to the path of the polkadot-sdk workspace.",
    )
}

fn build_nextest_command() -> String {
    "export PATH=/workspace/target/release:$PATH && \
        cd /workspace && \
        cargo nextest run \
        --archive-file /archive.tar.zst \
        --workspace-remap /workspace \
        --no-capture; \
        echo ''; \
        echo '=== Tests completed ==='"
        .to_string()
}

fn build_docker_command(archive_path: &Path, workspace_path: &str, inner_cmd: &str) -> Command {
    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "-it",
        "--rm",
        "-v",
        &format!("{}:/archive.tar.zst:ro", archive_path.display()),
        "-v",
        &format!("{}:/workspace", workspace_path),
        "-e",
        "ZOMBIE_PROVIDER=native",
        "-e",
        "RUST_LOG=debug",
        "docker.io/paritytech/ci-unified:bullseye-1.88.0-2025-06-27-v202506301118",
        "bash",
        "-c",
        inner_cmd,
    ]);
    cmd
}
