//! Live E2E tests: spawn real networks and check the diagnostics verdict for a
//! stalled parachain, a healthy network, and a paused node.
//!
//! `#[ignore]` (manual only, not CI): they need `polkadot` + `polkadot-parachain`
//! on `PATH` and take minutes. Run with:
//!
//! ```sh
//! cargo test -p zombie-mcp --test live_diagnostics -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use zombie_mcp::{
    diagnostics::{check_block_production, diagnose_run},
    input::{BlockProductionInput, DiagnoseRunInput},
    report::{Severity, Status},
};
use zombienet_sdk::{LocalFileSystem, Network, NetworkConfigBuilder, NetworkConfigExt};

const COLLATOR: &str = "people-collator";
const BEST_BLOCK_METRIC: &str = "block_height{status=\"best\"}";
/// How long to wait for a chain to start producing blocks.
const BLOCK_WAIT_SECS: u64 = 120;

fn spec_path() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/examples/people-westend-local-spec.json"
    ))
}

struct LiveNetwork {
    network: Network<LocalFileSystem>,
    zombie_json: PathBuf,
    base_dir: PathBuf,
}

impl LiveNetwork {
    /// Tear the network down and remove its base dir. Call before asserting so a
    /// panic cannot leak node processes.
    async fn cleanup(self) {
        let _ = self.network.destroy().await;
        let _ = std::fs::remove_dir_all(&self.base_dir);
    }
}

/// Spawn the people-westend network registering the parachain under `para_id`.
/// A `para_id` matching `SPEC_PARA_ID` is healthy; any other id leaves the
/// collator unbacked (spawned but stalled).
async fn spawn_people_network(suffix: &str, para_id: u32) -> anyhow::Result<LiveNetwork> {
    let base_dir = std::env::temp_dir().join(format!(
        "zombie-mcp-live-{suffix}-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let base_dir_str = base_dir.to_str().expect("temp base dir is valid UTF-8");
    let spec = spec_path();

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("westend-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_default_args(vec!["-lparachain=debug".into()])
                .with_validator(|n| n.with_name("validator-0"))
                .with_validator(|n| n.with_name("validator-1"))
        })
        .with_parachain(|p| {
            p.with_id(para_id)
                .with_chain_spec_path(spec.to_str().expect("spec path is valid UTF-8"))
                .with_default_command("polkadot-parachain")
                .with_default_image("docker.io/parity/polkadot-parachain:latest")
                .with_collator(|n| n.with_name(COLLATOR))
        })
        .with_global_settings(|g| {
            g.with_base_dir(base_dir_str)
                .with_tear_down_on_failure(false)
        })
        .build()
        .map_err(|errs| anyhow::anyhow!("config errors: {errs:?}"))?;

    let network = config.spawn_native().await?;
    let zombie_json = base_dir.join("zombie.json");
    Ok(LiveNetwork {
        network,
        zombie_json,
        base_dir,
    })
}

/// Spawn a relay-only network (two validators, no parachain). It reliably
/// produces blocks on `westend-local`, so it is the basis for the healthy
/// control and the node-down case.
async fn spawn_relay_only(suffix: &str) -> anyhow::Result<LiveNetwork> {
    let base_dir = std::env::temp_dir().join(format!(
        "zombie-mcp-live-{suffix}-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let base_dir_str = base_dir.to_str().expect("temp base dir is valid UTF-8");

    let config = NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("westend-local")
                .with_default_command("polkadot")
                .with_default_image("docker.io/parity/polkadot:latest")
                .with_validator(|n| n.with_name("validator-0"))
                .with_validator(|n| n.with_name("validator-1"))
        })
        .with_global_settings(|g| {
            g.with_base_dir(base_dir_str)
                .with_tear_down_on_failure(false)
        })
        .build()
        .map_err(|errs| anyhow::anyhow!("config errors: {errs:?}"))?;

    let network = config.spawn_native().await?;
    let zombie_json = base_dir.join("zombie.json");
    Ok(LiveNetwork {
        network,
        zombie_json,
        base_dir,
    })
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real network; requires polkadot + polkadot-parachain on PATH"]
async fn diagnoses_stalled_parachain_with_zombie_json_present() -> anyhow::Result<()> {
    // Register under the wrong id: every process starts and zombie.json is
    // written, but the collator is never backed and never produces blocks.
    let live = spawn_people_network("stalled", 2000).await?;
    let zombie_json_written = live.zombie_json.is_file();

    let report = diagnose_run(DiagnoseRunInput {
        zombie_json_path: live.zombie_json.clone(),
    })
    .await;
    let collator_stalled = report.evidence.iter().any(|e| {
        e.id == format!("node.{COLLATOR}.no_block_progress") && e.severity == Severity::Warning
    });
    let diagnose_status = report.status;

    // check_block_production must not count the genesis block as production.
    let block_report = check_block_production(BlockProductionInput {
        zombie_json_path: live.zombie_json.clone(),
        node_name: COLLATOR.to_string(),
        blocks: 1,
        timeout_secs: 15,
    })
    .await;
    let block_timeout = block_report.evidence.iter().any(|e| {
        e.id == format!("node.{COLLATOR}.blocks.timeout") && e.severity == Severity::Error
    });
    let block_status = block_report.status;

    live.cleanup().await;

    assert!(zombie_json_written, "zombie.json should be written after spawn");
    assert!(
        collator_stalled,
        "diagnose_run should flag no_block_progress for the collator: {:?}",
        report.evidence
    );
    assert_ne!(
        diagnose_status,
        Status::Ok,
        "diagnose_run must not report a stalled network as ok: {:?}",
        report.evidence
    );
    assert_eq!(
        block_status,
        Status::Failed,
        "check_block_production should fail on a stalled chain: {:?}",
        block_report.evidence
    );
    assert!(
        block_timeout,
        "check_block_production should time out instead of counting genesis: {:?}",
        block_report.evidence
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real network; requires polkadot + polkadot-parachain on PATH"]
async fn healthy_network_diagnoses_clean() -> anyhow::Result<()> {
    // Relay-only: a network that genuinely produces blocks, so the control is
    // about "healthy diagnoses clean", not about parachain backing.
    let live = spawn_relay_only("healthy").await?;

    // Wait until the relay is actually producing before diagnosing, otherwise a
    // validator would momentarily look stalled at genesis.
    let produced = live
        .network
        .get_node("validator-0")?
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |height| height >= 1.0, BLOCK_WAIT_SECS)
        .await;

    let report = diagnose_run(DiagnoseRunInput {
        zombie_json_path: live.zombie_json.clone(),
    })
    .await;
    let any_no_progress = report
        .evidence
        .iter()
        .any(|e| e.id.ends_with(".no_block_progress"));
    let any_error = report
        .evidence
        .iter()
        .any(|e| e.severity == Severity::Error);
    let status = report.status;

    live.cleanup().await;

    produced.map_err(|e| anyhow::anyhow!("parachain did not produce blocks in time: {e}"))?;
    assert!(
        !any_no_progress,
        "a healthy network must not be flagged with no_block_progress: {:?}",
        report.evidence
    );
    assert!(
        !any_error,
        "a healthy network must not produce error evidence: {:?}",
        report.evidence
    );
    assert_eq!(
        status,
        Status::Ok,
        "a healthy network should diagnose as ok: {:?}",
        report.evidence
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real network; requires polkadot + polkadot-parachain on PATH"]
async fn paused_node_is_reported_as_unresponsive() -> anyhow::Result<()> {
    let downed = "validator-1";
    let live = spawn_relay_only("node-down").await?;

    // Bring the network to a healthy state first, so the paused node is the only
    // anomaly the diagnostics should surface.
    let _ = live
        .network
        .get_node("validator-0")?
        .wait_metric_with_timeout(BEST_BLOCK_METRIC, |height| height >= 1.0, BLOCK_WAIT_SECS)
        .await;

    // Pause (SIGSTOP) the validator so its RPC stops answering.
    live.network.get_node(downed)?.pause().await?;

    let report = diagnose_run(DiagnoseRunInput {
        zombie_json_path: live.zombie_json.clone(),
    })
    .await;
    let downed_rpc_error = report.evidence.iter().any(|e| {
        e.id.starts_with(&format!("node.{downed}.rpc_")) && e.severity == Severity::Error
    });
    let status = report.status;
    let next_steps = report.next_steps.clone();

    live.cleanup().await;

    assert!(
        downed_rpc_error,
        "diagnose_run should flag the paused node's RPC as down: {:?}",
        report.evidence
    );
    assert_eq!(
        status,
        Status::Failed,
        "a network with a downed node should diagnose as failed: {:?}",
        report.evidence
    );
    assert!(
        next_steps.iter().any(|s| s.contains("RPC port")),
        "next steps should point at the RPC endpoint: {next_steps:?}"
    );

    Ok(())
}
