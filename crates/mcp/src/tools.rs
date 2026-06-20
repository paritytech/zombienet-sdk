use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

use crate::{
    diagnostics,
    input::{
        BlockProductionInput, ConfigInput, DiagnoseRunInput, ListNodesInput, MetricInput,
        NodeInput, NodeLogsInput,
    },
    recent_runs,
    server::ZombienetMcpServer,
};

impl ZombienetMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl ZombienetMcpServer {
    #[tool(
        description = "Find recent Zombienet run candidates without arguments. Use this first when the user asks to debug a run but did not provide zombie_json_path."
    )]
    pub fn find_recent_runs(&self) -> String {
        serde_json::to_string(&recent_runs::find_recent_runs())
            .expect("recent run reports should serialize")
    }

    #[tool(
        description = "Diagnose an already-started Zombienet run from zombie_json_path. If zombie.json is missing because startup failed early, logs near that expected path are scanned. Use after find_recent_runs when the user did not provide a path."
    )]
    pub async fn diagnose_run(&self, Parameters(input): Parameters<DiagnoseRunInput>) -> String {
        report_json(&diagnostics::diagnose_run(input).await)
    }

    #[tool(description = "Validate a Zombienet configuration file")]
    pub fn validate_config(&self, Parameters(input): Parameters<ConfigInput>) -> String {
        report_json(&diagnostics::validate_config(input))
    }

    #[tool(description = "List nodes from an attached live Zombienet network")]
    pub async fn list_nodes(&self, Parameters(input): Parameters<ListNodesInput>) -> String {
        report_json(&diagnostics::list_nodes(input).await)
    }

    #[tool(description = "Read recent logs for a node from an attached live Zombienet network")]
    pub async fn get_node_logs(&self, Parameters(input): Parameters<NodeLogsInput>) -> String {
        report_json(&diagnostics::get_node_logs(input).await)
    }

    #[tool(description = "Check whether a node RPC endpoint and process metric are responsive")]
    pub async fn check_node_liveness(&self, Parameters(input): Parameters<NodeInput>) -> String {
        report_json(&diagnostics::check_node_liveness(input).await)
    }

    #[tool(description = "Observe finalized block production for a node")]
    pub async fn check_block_production(
        &self,
        Parameters(input): Parameters<BlockProductionInput>,
    ) -> String {
        report_json(&diagnostics::check_block_production(input).await)
    }

    #[tool(description = "Query a Prometheus metric from a node")]
    pub async fn query_metric(&self, Parameters(input): Parameters<MetricInput>) -> String {
        report_json(&diagnostics::query_metric(input).await)
    }
}

fn report_json(report: &crate::report::DiagnosticReport) -> String {
    serde_json::to_string(report).expect("diagnostic reports should serialize")
}
