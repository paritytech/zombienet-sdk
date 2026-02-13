use serde::{Deserialize, Serialize};

use crate::shared::types::Port;

const DEFAULT_PROMETHEUS_IMAGE: &str = "prom/prometheus:latest";
const DEFAULT_GRAFANA_IMAGE: &str = "grafana/grafana:latest";

/// Configuration for the observability stack (Prometheus + Grafana)
///
/// When enabled, Docker/Podman containers are spawned after the network is up,
/// auto-configured to scrape all nodes' Prometheus metrics endpoints
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// Whether the observability stack is enabled
    #[serde(default)]
    enabled: bool,
    /// Host port to expose Prometheus on. If `None`, a random available port is used
    #[serde(default)]
    prometheus_port: Option<Port>,
    /// Host port to expose Grafana on. If `None`, a random available port is used
    #[serde(default)]
    grafana_port: Option<Port>,
    /// Docker image for Prometheus
    #[serde(default = "default_prometheus_image")]
    prometheus_image: String,
    /// Docker image for Grafana
    #[serde(default = "default_grafana_image")]
    grafana_image: String,
}

fn default_prometheus_image() -> String {
    DEFAULT_PROMETHEUS_IMAGE.to_string()
}

fn default_grafana_image() -> String {
    DEFAULT_GRAFANA_IMAGE.to_string()
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            prometheus_port: None,
            grafana_port: None,
            prometheus_image: default_prometheus_image(),
            grafana_image: default_grafana_image(),
        }
    }
}

impl ObservabilityConfig {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn prometheus_port(&self) -> Option<Port> {
        self.prometheus_port
    }

    pub fn grafana_port(&self) -> Option<Port> {
        self.grafana_port
    }

    pub fn prometheus_image(&self) -> &str {
        &self.prometheus_image
    }

    pub fn grafana_image(&self) -> &str {
        &self.grafana_image
    }
}

/// Builder for [`ObservabilityConfig`]
#[derive(Default)]
pub struct ObservabilityConfigBuilder {
    config: ObservabilityConfig,
}

impl ObservabilityConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable the observability stack
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    /// Set the host port for Prometheus
    pub fn with_prometheus_port(mut self, port: Port) -> Self {
        self.config.prometheus_port = Some(port);
        self
    }

    /// Set the host port for Grafana
    pub fn with_grafana_port(mut self, port: Port) -> Self {
        self.config.grafana_port = Some(port);
        self
    }

    /// Set a custom Prometheus Docker image
    pub fn with_prometheus_image(mut self, image: impl Into<String>) -> Self {
        self.config.prometheus_image = image.into();
        self
    }

    /// Set a custom Grafana Docker image
    pub fn with_grafana_image(mut self, image: impl Into<String>) -> Self {
        self.config.grafana_image = image.into();
        self
    }

    pub fn build(self) -> ObservabilityConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_disabled() {
        let config = ObservabilityConfig::default();
        assert!(!config.enabled());
        assert_eq!(config.prometheus_port(), None);
        assert_eq!(config.grafana_port(), None);
        assert_eq!(config.prometheus_image(), "prom/prometheus:latest");
        assert_eq!(config.grafana_image(), "grafana/grafana:latest");
    }

    #[test]
    fn builder_defaults_are_disabled() {
        let config = ObservabilityConfigBuilder::new().build();
        assert!(!config.enabled());
        assert_eq!(config.prometheus_port(), None);
        assert_eq!(config.grafana_port(), None);
    }

    #[test]
    fn builder_with_all_fields() {
        let config = ObservabilityConfigBuilder::new()
            .with_enabled(true)
            .with_prometheus_port(9090)
            .with_grafana_port(3000)
            .with_prometheus_image("prom/prometheus:v2.50.0")
            .with_grafana_image("grafana/grafana:10.0.0")
            .build();

        assert!(config.enabled());
        assert_eq!(config.prometheus_port(), Some(9090));
        assert_eq!(config.grafana_port(), Some(3000));
        assert_eq!(config.prometheus_image(), "prom/prometheus:v2.50.0");
        assert_eq!(config.grafana_image(), "grafana/grafana:10.0.0");
    }

    #[test]
    fn toml_round_trip() {
        let config = ObservabilityConfigBuilder::new()
            .with_enabled(true)
            .with_prometheus_port(9090)
            .with_grafana_port(3000)
            .build();

        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: ObservabilityConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn deserialize_from_toml_string() {
        let toml_str = r#"
            enabled = true
            prometheus_port = 9090
            grafana_port = 3000
            prometheus_image = "prom/prometheus:v2.50.0"
        "#;

        let config: ObservabilityConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled());
        assert_eq!(config.prometheus_port(), Some(9090));
        assert_eq!(config.grafana_port(), Some(3000));
        assert_eq!(config.prometheus_image(), "prom/prometheus:v2.50.0");
        assert_eq!(config.grafana_image(), "grafana/grafana:latest");
    }

    #[test]
    fn deserialize_empty_toml_defaults_to_disabled() {
        let config: ObservabilityConfig = toml::from_str("").unwrap();
        assert!(!config.enabled());
    }
}
