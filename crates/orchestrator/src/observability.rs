use std::path::Path;

use configuration::ObservabilityConfig;
use support::fs::FileSystem;
use tokio::process::Command;
use tracing::{debug, trace};

use crate::{generators, network::node::NetworkNode};

/// Lifecycle state of the observability stack within a network
#[derive(Debug, Clone, Default)]
pub enum ObservabilityState {
    /// Stack has never been started
    #[default]
    NotStarted,
    /// Stack is running
    Running(ObservabilityInfo),
    /// Stack was running bu has been stopped
    Stopped,
}

/// Information about a running observability stack
#[derive(Debug, Clone)]
pub struct ObservabilityInfo {
    /// URL where Prometheus is accessible
    pub prometheus_url: String,
    /// URL where Grafana is accessible
    pub grafana_url: String,
    /// Name of the Prometheus Docker container
    prometheus_container_name: String,
    /// Name of the Grafana Docker container
    grafana_container_name: String,
    /// Container runtime binary (docker or podman)
    container_runtime: String,
}

impl ObservabilityState {
    pub fn as_runnnig(&self) -> Option<&ObservabilityInfo> {
        match self {
            ObservabilityState::Running(info) => Some(info),
            _ => None,
        }
    }
}

/// Spawn the observability stack (Prometheus + Grafana) as Docker/Podman containers
pub async fn spawn_observability_stack<T: FileSystem>(
    config: &ObservabilityConfig,
    nodes: &[&NetworkNode],
    ns_name: &str,
    base_dir: &Path,
    filesystem: &T,
) -> Result<ObservabilityInfo, anyhow::Error> {
    let container_runtime = detect_container_runtime().await?;
    debug!("Using container runtime: {container_runtime}");

    let (host_addr, use_host_network) = get_networking();

    let prom_parked = generators::generate_node_port(config.prometheus_port())?;
    let prom_port = prom_parked.0;
    let grafana_parked = generators::generate_node_port(config.grafana_port())?;
    let grafana_port = grafana_parked.0;

    // Create dirs
    let obs_dir = base_dir.join("observability");
    let prom_dir = obs_dir.join("prometheus");
    let grafana_ds_dir = obs_dir.join("grafana/provisioning/datasources");
    filesystem.create_dir_all(&prom_dir).await?;
    filesystem.create_dir_all(&grafana_ds_dir).await?;

    // Generate and write Prometheus config
    let prom_config = generate_prometheus_config(nodes, &host_addr);
    trace!("Generated prometheus.yml:\n{prom_config}");
    filesystem
        .write(prom_dir.join("prometheus.yml"), prom_config.as_bytes())
        .await?;

    let prom_url_for_grafana = if use_host_network {
        format!("http://127.0.0.1:{prom_port}")
    } else {
        format!("http://{host_addr}:{prom_port}")
    };

    let grafana_ds = generate_grafana_datasource(&prom_url_for_grafana);
    trace!("Generated grafana datasource.yml:\n{grafana_ds}");
    filesystem
        .write(grafana_ds_dir.join("datasource.yml"), grafana_ds.as_bytes())
        .await?;

    let prom_container = format!("{ns_name}-prometheus");
    let grafana_container = format!("{ns_name}-grafana");

    let mut prom_cmd = Command::new(&container_runtime);
    prom_cmd.args([
        "run",
        "-d",
        "--name",
        &prom_container,
        "-v",
        &format!("{}:/etc/prometheus", prom_dir.display()),
    ]);

    if use_host_network {
        prom_cmd.args(["--network=host"]);
    } else {
        prom_cmd.args(["-p", &format!("{prom_port}:9090")]);
    }

    prom_parked.drop_listener();

    prom_cmd.args([
        config.prometheus_image(),
        "--config.file=/etc/prometheus/prometheus.yml",
        "--storage.tsdb.path=/prometheus",
    ]);

    if use_host_network {
        // When using host network, override the listen address to use the assigned port
        prom_cmd.arg(format!("--web.listen-address=0.0.0.0:{prom_port}"));
    }

    let output = prom_cmd.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to start Prometheus container: {stderr}"
        ));
    }
    debug!("Prometheus container started: {prom_container}");

    let mut grafana_cmd = Command::new(&container_runtime);
    grafana_cmd.args([
        "run",
        "-d",
        "--name",
        &grafana_container,
        "-v",
        &format!(
            "{}:/etc/grafana/provisioning",
            obs_dir.join("grafana/provisioning").display()
        ),
        "-e",
        "GF_AUTH_ANONYMOUS_ENABLED=true",
        "-e",
        "GF_AUTH_ANONYMOUS_ORG_ROLE=Admin",
        "-e",
        "GF_SECURITY_ADMIN_PASSWORD=admin",
    ]);

    if use_host_network {
        grafana_cmd.args(["--network=host"]);
        grafana_cmd.args(["-e", &format!("GF_SERVER_HTTP_PORT={grafana_port}")]);
    } else {
        grafana_cmd.args(["-p", &format!("{grafana_port}:3000")]);
    }

    grafana_cmd.arg(config.grafana_image());

    grafana_parked.drop_listener();

    let output = grafana_cmd.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Clean up Prometheus container on failure
        let _ = Command::new(&container_runtime)
            .args(["rm", "--force", &prom_container])
            .output()
            .await;
        return Err(anyhow::anyhow!(
            "Failed to start Grafana container: {stderr}"
        ));
    }
    debug!("Grafana container started: {grafana_container}");

    Ok(ObservabilityInfo {
        prometheus_url: format!("http://127.0.0.1:{prom_port}"),
        grafana_url: format!("http://127.0.0.1:{grafana_port}"),
        prometheus_container_name: prom_container,
        grafana_container_name: grafana_container,
        container_runtime,
    })
}

pub async fn cleanup_observability_stack(info: &ObservabilityInfo) -> Result<(), anyhow::Error> {
    for container in [
        &info.prometheus_container_name,
        &info.grafana_container_name,
    ] {
        let output = Command::new(&info.container_runtime)
            .args(["rm", "--force", container])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("Warning: failed to remove container {container}: {stderr}");
        }
    }

    Ok(())
}

pub(crate) fn generate_prometheus_config(nodes: &[&NetworkNode], host_addr: &str) -> String {
    let mut targets = String::new();

    for node in nodes {
        if let Some(port) = extract_port_from_uri(&node.prometheus_uri) {
            targets.push_str(&format!(
                "      - targets: ['{host_addr}:{port}']\n        labels:\n          node: '{}'\n",
                node.name
            ));
        }
    }

    format!(
        "global:\n  scrape_interval: 5s\n  evaluation_interval: 5s\n\nscrape_configs:\n  - job_name: 'zombienet'\n    metrics_path: /metrics\n    static_configs:\n{targets}"
    )
}

pub(crate) fn generate_grafana_datasource(prom_url: &str) -> String {
    format!(
        "apiVersion: 1\ndatasources:\n  - name: Prometheus\n    type: prometheus\n    access: proxy\n    url: {prom_url}\n    isDefault: true\n    editable: true\n"
    )
}

async fn detect_container_runtime() -> Result<String, anyhow::Error> {
    if let Ok(output) = Command::new("docker").arg("version").output().await {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.to_lowercase().contains("podman") {
                return Ok("podman".to_string());
            }
            return Ok("docker".to_string());
        }
    }

    if let Ok(output) = Command::new("podman").arg("version").output().await {
        if output.status.success() {
            return Ok("podman".to_string());
        }
    }

    Err(anyhow::anyhow!(
        "No container runtime found. Install Docker or Podman to use the observability stack."
    ))
}

fn get_networking() -> (String, bool) {
    match std::env::consts::OS {
        "linux" => ("127.0.0.1".to_string(), true),
        _ => ("host.docker.internal".to_string(), false),
    }
}

fn extract_port_from_uri(uri: &str) -> Option<u16> {
    uri.rsplit(':')
        .next()
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.parse().ok())
}
