#![cfg(feature = "mcp")]

use std::{borrow::Cow, fs, path::PathBuf, time::Duration};

use rmcp::{
    model::{CallToolRequestParams, JsonObject},
    serve_client,
    service::{RoleClient, RunningService},
    transport::TokioChildProcess,
};
use serde_json::Value;
use tokio::{process::Command, time::timeout};

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn stdio_tool_discovery_includes_diagnose_run() -> anyhow::Result<()> {
    let mut client = timeout(TEST_TIMEOUT, spawn_client()).await??;

    let tools_result = timeout(TEST_TIMEOUT, client.peer().list_all_tools()).await;
    let close_result = close_client(&mut client).await;

    let tools = tools_result??;
    close_result?;

    assert!(
        tools.iter().any(|tool| tool.name == "diagnose_run"),
        "expected diagnose_run in discovered tools: {tools:?}",
    );
    assert!(
        tools.iter().any(|tool| tool.name == "find_recent_runs"),
        "expected find_recent_runs in discovered tools: {tools:?}",
    );

    Ok(())
}

#[tokio::test]
async fn find_recent_runs_works_without_arguments_over_stdio() -> anyhow::Result<()> {
    let run_dir = std::env::temp_dir().join(format!(
        "zombienet-mcp-stdio-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let node_dir = run_dir.join("alice");
    fs::create_dir_all(&node_dir)?;
    fs::write(node_dir.join("alice.log"), "startup failed")?;

    let mut client = timeout(TEST_TIMEOUT, spawn_client()).await??;

    let call_result = timeout(
        TEST_TIMEOUT,
        client.peer().call_tool(CallToolRequestParams {
            meta: None,
            name: Cow::Borrowed("find_recent_runs"),
            arguments: None,
            task: None,
        }),
    )
    .await;
    let close_result = close_client(&mut client).await;

    let result = call_result??;
    close_result?;
    fs::remove_dir_all(&run_dir)?;

    let response_text = result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.as_str()))
        .collect::<String>();

    assert!(
        response_text.contains(&run_dir.join("zombie.json").display().to_string()),
        "expected synthesized zombie_json_path in find_recent_runs response: {response_text}",
    );

    Ok(())
}

#[tokio::test]
async fn validate_config_reports_invalid_fixture_over_stdio() -> anyhow::Result<()> {
    let mut client = timeout(TEST_TIMEOUT, spawn_client()).await??;
    let mut arguments = JsonObject::new();
    arguments.insert(
        "config_path".to_string(),
        Value::String(fixture_path("invalid.toml").display().to_string()),
    );

    let call_result = timeout(
        TEST_TIMEOUT,
        client.peer().call_tool(CallToolRequestParams {
            meta: None,
            name: Cow::Borrowed("validate_config"),
            arguments: Some(arguments),
            task: None,
        }),
    )
    .await;
    let close_result = close_client(&mut client).await;

    let result = call_result??;
    close_result?;
    let response_text = result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.as_str()))
        .collect::<String>();

    assert!(
        response_text.contains("config.invalid"),
        "expected config.invalid in validate_config response: {response_text}",
    );

    Ok(())
}

async fn spawn_client() -> anyhow::Result<RunningService<RoleClient, ()>> {
    let transport = TokioChildProcess::new(Command::new(env!("CARGO_BIN_EXE_zombie-mcp")))?;
    Ok(serve_client((), transport).await?)
}

async fn close_client(client: &mut RunningService<RoleClient, ()>) -> anyhow::Result<()> {
    timeout(TEST_TIMEOUT, client.close()).await??;
    Ok(())
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}
