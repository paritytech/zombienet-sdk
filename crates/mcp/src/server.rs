use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool_handler, ServerHandler,
};

#[derive(Clone)]
pub struct ZombienetMcpServer {
    pub(crate) tool_router: ToolRouter<Self>,
}

impl Default for ZombienetMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ZombienetMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "This is a read-only Zombienet diagnostics server. When the user asks to debug a run without giving a path, call find_recent_runs first, then diagnose_run with the newest zombie_json_path. It inspects configs, logs, live nodes, and metrics, and never spawns or destroys networks.".to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use rmcp::ServerHandler;

    use super::ZombienetMcpServer;

    #[test]
    fn exposes_diagnostic_tools_with_stable_names() {
        let server = ZombienetMcpServer::new();
        let mut names = server
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(
            names,
            vec![
                "check_block_production",
                "check_node_liveness",
                "diagnose_run",
                "find_recent_runs",
                "get_node_logs",
                "list_nodes",
                "query_metric",
                "validate_config",
            ],
        );
    }

    #[test]
    fn server_info_describes_read_only_diagnostics() {
        let info = ZombienetMcpServer::new().get_info();
        let instructions = info
            .instructions
            .expect("server should provide client instructions");

        assert!(instructions.contains("read-only"));
        assert!(instructions.contains("never spawns"));
        assert!(instructions.contains("destroys networks"));
        assert!(info.capabilities.tools.is_some());
    }
}
