//! Live integration test for the "zombie.json exists but the parachain is
//! stalled" failure mode.
//!
//! It spawns a real network whose parachain is registered under the wrong para
//! id: the chain spec is built for para 1004, but it is registered as 2000. Every
//! process therefore starts and `zombie.json` is written, yet the collator is
//! never backed and never produces blocks. The test then asserts that the MCP
//! diagnostics surface the stall instead of reporting the run as healthy.
//!
//! Ignored by default: it requires `polkadot` and `polkadot-parachain` on `PATH`
//! and takes a couple of minutes. Run it explicitly with:
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
use zombienet_sdk::{NetworkConfigBuilder, NetworkConfigExt};

const COLLATOR: &str = "people-collator";
/// The chain spec is built for para 1004; registering it under a different id
/// keeps every process alive while preventing block production.
const REGISTERED_PARA_ID: u32 = 2000;

fn spec_path() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/examples/people-westend-local-spec.json"
    ))
}

fn binary_on_path(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a real network; requires polkadot + polkadot-parachain on PATH"]
async fn diagnoses_stalled_parachain_with_zombie_json_present() -> anyhow::Result<()> {
    if !binary_on_path("polkadot") || !binary_on_path("polkadot-parachain") {
        eprintln!("skipping: polkadot/polkadot-parachain not found on PATH");
        return Ok(());
    }
    let spec = spec_path();
    if !spec.is_file() {
        eprintln!("skipping: chain spec not found at {}", spec.display());
        return Ok(());
    }

    let base_dir = std::env::temp_dir().join(format!(
        "zombie-mcp-live-stalled-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let base_dir_str = base_dir.to_str().expect("temp base dir is valid UTF-8");

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
            p.with_id(REGISTERED_PARA_ID)
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

    let zombie_json_path = base_dir.join("zombie.json");
    let zombie_json_written = zombie_json_path.is_file();

    // diagnose_run must flag the stalled collator instead of reporting "ok".
    let report = diagnose_run(DiagnoseRunInput {
        zombie_json_path: zombie_json_path.clone(),
    })
    .await;
    let collator_stalled = report.evidence.iter().any(|e| {
        e.id == format!("node.{COLLATOR}.no_block_progress") && e.severity == Severity::Warning
    });
    let diagnose_status = report.status;

    // check_block_production must not count the genesis block as production.
    let block_report = check_block_production(BlockProductionInput {
        zombie_json_path: zombie_json_path.clone(),
        node_name: COLLATOR.to_string(),
        blocks: 1,
        timeout_secs: 15,
    })
    .await;
    let block_timeout = block_report.evidence.iter().any(|e| {
        e.id == format!("node.{COLLATOR}.blocks.timeout") && e.severity == Severity::Error
    });
    let block_status = block_report.status;

    // Always tear the network down before asserting, so a failure cannot leak
    // node processes.
    let _ = network.destroy().await;
    let _ = std::fs::remove_dir_all(&base_dir);

    assert!(
        zombie_json_written,
        "zombie.json should be written after spawn"
    );
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
