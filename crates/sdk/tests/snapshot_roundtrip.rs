//! End-to-end producer → consumer roundtrip for the snapshot API.
//!
//! 1. Spawn a relay (2 validators) + parachain (2 collators) network.
//! 2. Wait until both chains are producing AND finalizing blocks.
//! 3. Capture the parachain height; pause all nodes; take per-node DB
//!    snapshots (relay from a validator, para from a collator); bundle them.
//! 4. Tear the network down.
//! 5. Unpack the bundle, verify checksums and manifest contents.
//! 6. Spawn a NEW network where each chain loads its snapshot via
//!    `with_default_db_snapshot` — both validators share the relay archive,
//!    both collators share the para archive.
//! 7. Wait until both chains progress past their snapshot height AND keep
//!    finalizing — proves snapshot extraction works, identity-stripping
//!    worked (no equivocation stall), and the orchestrator's race-free
//!    asset resolution holds when sibling nodes share one archive.
//!
//! Marked `#[ignore]` because it spawns real polkadot binaries; run with
//! `cargo test --test snapshot_roundtrip -- --ignored --nocapture`.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use configuration::{NetworkConfig, NetworkConfigBuilder};
use serde_json::json;
use zombienet_sdk::{
    environment::get_spawn_fn, snapshot::untar_bundle, BundleBuilder, NetworkConfigExt,
    NetworkNode, SnapshotManifest,
};

const BEST_BLOCK: &str = "block_height{status=\"best\"}";
const FINALIZED_BLOCK: &str = "block_height{status=\"finalized\"}";
const PARA_ID: u32 = 2000;

/// Per-chain height the producer must reach (best) before we snapshot.
const PRODUCER_BEST: f64 = 16.0;
/// Per-chain finalized height the producer must reach before we snapshot —
/// confirms GRANDPA/para-finality is live, not just block authoring.
const PRODUCER_FINALIZED: f64 = 12.0;
/// How many blocks past the snapshot height the consumer must advance.
const RESUME_DELTA: f64 = 4.0;

const READY_TIMEOUT_SECS: u64 = 120;
const PROGRESS_TIMEOUT_SECS: u64 = 300;

/// 2 relay validators + 2 collators. When `snapshots` is `Some((relay, para))`,
/// every validator loads the relay archive and every collator the para
/// archive — the consumer side. `None` is the producer side (fresh chain).
fn network(snapshots: Option<(&Path, &Path)>) -> NetworkConfig {
    let relay_snap = snapshots.map(|(r, _)| r.to_string_lossy().into_owned());
    let para_snap = snapshots.map(|(_, p)| p.to_string_lossy().into_owned());

    NetworkConfigBuilder::new()
        .with_relaychain(|r| {
            r.with_chain("rococo-local")
                .with_default_command("polkadot")
                .with_optional_default_db_snapshot(relay_snap.as_deref())
                .with_validator(|node| node.with_name("alice"))
                .with_validator(|node| node.with_name("bob"))
        })
        .with_parachain(|p| {
            p.with_id(PARA_ID)
                .cumulus_based(true)
                .with_default_command("polkadot-parachain")
                .with_optional_default_db_snapshot(para_snap.as_deref())
                .with_collator(|n| n.with_name("collator-1"))
                .with_collator(|n| n.with_name("collator-2"))
        })
        .build()
        .expect("NetworkConfig builds")
}

/// Wait until `node` reports best ≥ `best` AND finalized ≥ `finalized`.
async fn wait_progress_and_finality(
    node: &NetworkNode,
    best: f64,
    finalized: f64,
    timeout: u64,
) -> anyhow::Result<()> {
    node.wait_metric_with_timeout(BEST_BLOCK, move |x| x >= best, timeout)
        .await?;
    node.wait_metric_with_timeout(FINALIZED_BLOCK, move |x| x >= finalized, timeout)
        .await?;
    Ok(())
}

fn sha256_of(path: &Path) -> anyhow::Result<String> {
    use sha2::Digest;
    let bytes = std::fs::read(path)?;
    Ok(hex::encode(sha2::Sha256::digest(&bytes)))
}

/// Anchor heights captured from the producer at snapshot time.
struct Anchors {
    relay: u64,
    para: u64,
}

/// Spawn the producer network, wait for both chains to progress + finalize,
/// snapshot a relay validator + a collator, and assemble the bundle.
/// Returns the bundle and the captured anchor heights.
async fn prepare_bundle(out_dir: &Path) -> (zombienet_sdk::Bundle, Anchors) {
    println!("🚀 spawning producer network");
    let spawn_fn = get_spawn_fn();
    let producer = spawn_fn(network(None))
        .await
        .expect("producer network spawns");
    producer
        .wait_until_is_up(READY_TIMEOUT_SECS)
        .await
        .expect("producer is up");

    let relay_node = producer.get_node("alice").expect("alice exists");
    let para_node = producer.get_node("collator-1").expect("collator-1 exists");

    println!("⏳ waiting for both chains to progress + finalize");
    wait_progress_and_finality(
        relay_node,
        PRODUCER_BEST,
        PRODUCER_FINALIZED,
        PROGRESS_TIMEOUT_SECS,
    )
    .await
    .expect("relay progresses + finalizes");
    wait_progress_and_finality(
        para_node,
        PRODUCER_BEST,
        PRODUCER_FINALIZED,
        PROGRESS_TIMEOUT_SECS,
    )
    .await
    .expect("para progresses + finalizes");

    // Capture heights *before* pausing (a paused process won't serve
    // Prometheus). These are the anchors the consumer must exceed.
    let anchors = Anchors {
        relay: relay_node.reports(BEST_BLOCK).await.expect("relay height") as u64,
        para: para_node.reports(BEST_BLOCK).await.expect("para height") as u64,
    };
    println!(
        "📏 snapshot anchors: relay={} para={}",
        anchors.relay, anchors.para
    );

    println!("⏸  pausing network");
    producer.pause().await.expect("pause");

    let relay_snap = relay_node
        .snapshot_db(out_dir.join("relaychain-db.tgz"))
        .await
        .expect("snapshot relay");
    let para_snap = para_node
        .snapshot_db(out_dir.join("parachain-db.tgz"))
        .await
        .expect("snapshot para");
    println!(
        "📦 produced {} ({} B) and {} ({} B)",
        relay_snap.path.display(),
        relay_snap.size,
        para_snap.path.display(),
        para_snap.size
    );

    let bundle = BundleBuilder::new()
        .add(relay_snap)
        .add(para_snap)
        .user_data(json!({
            "relay_best_block": anchors.relay,
            "para_best_block": anchors.para,
        }))
        .build(out_dir.join("bundle.tar.gz"))
        .expect("bundle builds");
    println!(
        "📦 bundle: {} ({} B, sha256 {})",
        bundle.path.display(),
        bundle.size,
        bundle.sha256
    );

    producer.destroy().await.expect("destroy producer");
    (bundle, anchors)
}

/// Verify the bundle: outer sha256 matches, members present, manifest
/// deserialises with the expected anchors and per-archive checksums.
/// Returns the paths of the two extracted inner archives.
fn verify_bundle(
    bundle: &zombienet_sdk::Bundle,
    out_dir: &Path,
    anchors: &Anchors,
) -> (PathBuf, PathBuf) {
    assert!(bundle.path.is_file(), "bundle file should exist");
    assert_eq!(
        sha256_of(&bundle.path).expect("hash bundle"),
        bundle.sha256,
        "Bundle.sha256 matches disk"
    );

    let extracted = out_dir.join("extracted");
    untar_bundle(&bundle.path, &extracted).expect("untar bundle");

    let inner_relay = extracted.join("relaychain-db.tgz");
    let inner_para = extracted.join("parachain-db.tgz");
    let inner_manifest = extracted.join("manifest.json");
    assert!(inner_relay.is_file(), "relay archive in bundle");
    assert!(inner_para.is_file(), "para archive in bundle");
    assert!(inner_manifest.is_file(), "manifest.json in bundle");

    let manifest: SnapshotManifest =
        serde_json::from_str(&std::fs::read_to_string(&inner_manifest).expect("read manifest"))
            .expect("manifest deserialises");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.archives.len(), 2);
    assert_eq!(
        manifest.user_data["para_best_block"].as_u64(),
        Some(anchors.para),
        "para height round-trips"
    );
    assert_eq!(
        manifest.user_data["relay_best_block"].as_u64(),
        Some(anchors.relay),
        "relay height round-trips"
    );
    for entry in &manifest.archives {
        let p = extracted.join(&entry.file);
        assert_eq!(
            sha256_of(&p).expect("hash inner archive"),
            entry.sha256,
            "manifest sha256 matches disk for {}",
            entry.file
        );
    }

    (inner_relay, inner_para)
}

/// Spawn a fresh network from the extracted archives and assert both chains
/// resume past their anchor heights AND keep finalizing.
async fn consume_bundle(out_dir: &Path, inner_relay: &Path, inner_para: &Path, anchors: &Anchors) {
    println!("🚀 spawning consumer network from snapshot");
    let consumer_relay = out_dir.join("consumer-relay.tgz");
    let consumer_para = out_dir.join("consumer-para.tgz");
    std::fs::copy(inner_relay, &consumer_relay).expect("stage relay");
    std::fs::copy(inner_para, &consumer_para).expect("stage para");

    let consumer = network(Some((&consumer_relay, &consumer_para)))
        .spawn_native()
        .await
        .expect("consumer network spawns from snapshot");
    consumer
        .wait_until_is_up(READY_TIMEOUT_SECS)
        .await
        .expect("consumer is up");

    // Both chains must resume from the snapshot (best advances past the
    // anchor) AND keep finalizing past the anchor — finalized lagged the
    // anchor at snapshot time, so reaching it proves real post-resume
    // finality, not just replayed state.
    let relay2 = consumer.get_node("alice").expect("alice in consumer");
    let para2 = consumer
        .get_node("collator-1")
        .expect("collator-1 in consumer");

    println!(
        "⏳ consumer relay must advance + finalize past {}",
        anchors.relay
    );
    wait_progress_and_finality(
        relay2,
        anchors.relay as f64 + RESUME_DELTA,
        anchors.relay as f64,
        PROGRESS_TIMEOUT_SECS,
    )
    .await
    .expect("consumer relay resumes + finalizes");

    println!(
        "⏳ consumer para must advance + finalize past {}",
        anchors.para
    );
    wait_progress_and_finality(
        para2,
        anchors.para as f64 + RESUME_DELTA,
        anchors.para as f64,
        PROGRESS_TIMEOUT_SECS,
    )
    .await
    .expect("consumer para resumes + finalizes");

    consumer.destroy().await.expect("destroy consumer");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns real polkadot binaries; run with --ignored"]
async fn snapshot_roundtrip() {
    tracing_subscriber::fmt::try_init().ok();

    let now = Instant::now();
    let out_dir = std::env::temp_dir().join(format!(
        "zombie-snapshot-roundtrip-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&out_dir).expect("create out_dir");
    println!("🗂  out_dir: {}", out_dir.display());

    let (bundle, anchors) = prepare_bundle(&out_dir).await;
    let (inner_relay, inner_para) = verify_bundle(&bundle, &out_dir, &anchors);
    consume_bundle(&out_dir, &inner_relay, &inner_para, &anchors).await;

    println!("🎉 roundtrip OK in {:.2?}", now.elapsed());
    // let _ = std::fs::remove_dir_all(&out_dir);
}
