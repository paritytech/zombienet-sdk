//! Starting the custom processes of a network once the nodes are up: each one
//! as soon as its dependencies have met their conditions, in parallel with
//! whatever else can start, `spawn_concurrency` at a time. A process that
//! does not meet its condition, or never starts, fails the spawn, unless it
//! is allowed to fail: then it is recorded on the network and the spawn goes
//! on.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use configuration::{CustomProcess, DependencyCondition};
use futures::{stream::FuturesUnordered, StreamExt};
use provider::{constants::LOCALHOST, ProviderNamespace};
use support::fs::FileSystem;
use tracing::{info, warn};

use crate::{
    errors::OrchestratorError,
    network::{node::CustomProcessState, Network},
    network_spec::NetworkSpec,
    spawner::{ports_json, prepare_process, spawn_process, PreparedProcess},
};

/// What the scheduler knows about a process so far.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Spawned; its state so far.
    Reached(CustomProcessState),
    /// Never spawned.
    Skipped,
}

/// Whether a process may start, given what its dependencies reached.
enum Startable {
    Now,
    Later,
    Never(String),
}

fn startable(
    process: &CustomProcess,
    by_name: &HashMap<&str, &CustomProcess>,
    phases: &HashMap<String, Phase>,
) -> Startable {
    for dep in process.depends_on() {
        let Some(target) = by_name.get(dep.name.as_str()) else {
            return Startable::Never(format!("depends on '{}', which was not spawned", dep.name));
        };
        let condition = dep.condition_on(target);
        let Some(phase) = phases.get(dep.name.as_str()) else {
            return Startable::Later;
        };
        let met = match (condition, phase) {
            (_, Phase::Skipped) => false,
            (DependencyCondition::ServiceStarted, _) => true,
            (DependencyCondition::ServiceHealthy, Phase::Reached(CustomProcessState::Ready)) => {
                true
            },
            (
                DependencyCondition::ServiceCompletedSuccessfully,
                Phase::Reached(CustomProcessState::Completed),
            ) => true,
            (_, Phase::Reached(CustomProcessState::Starting)) => return Startable::Later,
            _ => false,
        };
        if !met {
            return Startable::Never(format!(
                "depends on '{}' being {}, which it is not ({})",
                dep.name,
                condition.as_str(),
                match phase {
                    Phase::Reached(state) => state.to_string(),
                    Phase::Skipped => "skipped".into(),
                }
            ));
        }
    }
    Startable::Now
}

/// What a process that did not make it means for the spawn: the reason to
/// record when the process is allowed to fail, else the error that fails it.
fn tolerate(process: &CustomProcess, reason: String) -> Result<String, OrchestratorError> {
    if process.allowed_to_fail() {
        Ok(reason)
    } else {
        Err(OrchestratorError::CustomProcessFailed {
            name: process.name().to_string(),
            reason,
        })
    }
}

/// A process that is not spawned: fails the spawn, or, when allowed to fail,
/// is recorded on the network and known to the scheduler so its dependents
/// are refused too.
fn skip<T: FileSystem>(
    network: &mut Network<T>,
    phases: &mut HashMap<String, Phase>,
    process: &CustomProcess,
    reason: String,
) -> Result<(), OrchestratorError> {
    let reason = tolerate(process, reason)?;
    network.add_skipped_custom_process(process.name(), reason);
    phases.insert(process.name().to_string(), Phase::Skipped);
    Ok(())
}

/// Start each process once its dependencies are met, at most
/// `spawn_concurrency` spawning at a time. The first process that fails, or
/// is never started, without being allowed to fail fails the spawn.
pub(crate) async fn spawn_custom_processes<T: FileSystem>(
    network: &mut Network<T>,
    spec: &NetworkSpec,
    ns: Arc<dyn ProviderNamespace + Send + Sync>,
    spawn_concurrency: usize,
) -> Result<(), OrchestratorError> {
    if spec.custom_processes.is_empty() {
        return Ok(());
    }
    let host_ip = spec
        .global_settings
        .local_ip()
        .copied()
        .unwrap_or(LOCALHOST);
    let default_timeout = Duration::from_secs(u64::from(spec.global_settings.node_spawn_timeout()));
    let by_name: HashMap<&str, &CustomProcess> = spec
        .custom_processes
        .iter()
        .map(|cp| (cp.name(), cp))
        .collect();
    let mut phases: HashMap<String, Phase> = HashMap::new();

    // The loader and the builder refuse a name a node or process already has,
    // but a `NetworkConfig` or `NetworkSpec` deserialized directly skips them,
    // and a process registered under a node's name would replace the node.
    // Checked before spawning, so no process runs untracked. Every process
    // parks its ports before any spawns, see `PreparedProcess`.
    let mut pending: Vec<PreparedProcess> = vec![];
    let mut seen: HashSet<&str> = HashSet::new();
    for cp in &spec.custom_processes {
        if network.has_member(cp.name()) || !seen.insert(cp.name()) {
            skip(
                network,
                &mut phases,
                cp,
                "the name is already taken by a node or custom process".into(),
            )?;
            continue;
        }
        match prepare_process(cp, ns.as_ref()) {
            Ok(prepared) => pending.push(prepared),
            Err(e) => skip(
                network,
                &mut phases,
                cp,
                format!("could not reserve its ports: {e}"),
            )?,
        }
    }

    // Every process's ports are known now, so a process may name another's,
    // `{{ZOMBIE:<process>:port_<name>}}`, the way it names its own.
    let mut nodes_by_name = network.nodes_json()?;
    for prepared in &pending {
        nodes_by_name[prepared.spec.name()] = ports_json(&prepared.ports);
    }

    // Spawning and waiting are two steps, so a `service_started` dependent
    // goes as soon as its dependency spawned, while that one is still waited
    // on. The concurrency limit is on the spawning step, as for nodes.
    let mut spawning = FuturesUnordered::new();
    let mut waiting = FuturesUnordered::new();
    loop {
        let mut i = 0;
        while i < pending.len() {
            match startable(pending[i].spec, &by_name, &phases) {
                Startable::Now if spawning.len() >= spawn_concurrency => i += 1,
                Startable::Now => {
                    let prepared = pending.swap_remove(i);
                    let spec = prepared.spec;
                    let ns = ns.clone();
                    let nodes_by_name = &nodes_by_name;
                    spawning.push(async move {
                        let outcome = spawn_process(prepared, ns, host_ip, nodes_by_name).await;
                        (spec, outcome)
                    });
                },
                Startable::Later => i += 1,
                Startable::Never(reason) => {
                    let prepared = pending.swap_remove(i);
                    skip(network, &mut phases, prepared.spec, reason)?;
                },
            }
        }
        if spawning.is_empty() && waiting.is_empty() {
            break;
        }
        tokio::select! {
            Some((spec, outcome)) = spawning.next(), if !spawning.is_empty() => match outcome {
                Ok(node) => {
                    phases.insert(spec.name().to_string(), Phase::Reached(CustomProcessState::Starting));
                    let timeout = spec
                        .timeout()
                        .map(|secs| Duration::from_secs(u64::from(secs)))
                        .unwrap_or(default_timeout);
                    waiting.push(async move {
                        let state = node.wait_ready(timeout).await;
                        (node, state)
                    });
                },
                Err(e) => skip(network, &mut phases, spec, format!("failed to spawn: {e}"))?,
            },
            Some((mut node, state)) = waiting.next(), if !waiting.is_empty() => {
                let name = node.name().to_string();
                match &state {
                    CustomProcessState::Failed { reason } => {
                        let reason = tolerate(node.spec(), reason.clone())?;
                        warn!("⚠️  {name}: {reason}");
                    },
                    state => info!("✅ {name}: {state}"),
                }
                node.state = state.clone();
                phases.insert(name, Phase::Reached(state));
                network.add_running_custom_process(node);
            },
        }
    }
    // Only a cycle leaves something pending; the config check should have
    // caught it, so this is defensive.
    for prepared in pending {
        skip(
            network,
            &mut phases,
            prepared.spec,
            "its dependencies never started (cycle?)".into(),
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use configuration::CustomProcessBuilder;

    use super::*;

    #[test]
    fn startable_follows_the_dependency_conditions() {
        let mk = |name: &str,
                  deps: Vec<(&str, Option<DependencyCondition>)>,
                  one_shot: bool,
                  port: bool| {
            let mut b = CustomProcessBuilder::new()
                .with_name(name)
                .with_command("sh");
            for (dep, cond) in deps {
                b = b.with_dependency_on(dep, cond);
            }
            if one_shot {
                b = b.with_one_shot();
            }
            if port {
                b = b.with_named_port("p", 0);
            }
            b.build().unwrap()
        };
        let db = mk("db", vec![], false, true);
        let init = mk("init", vec![], true, false);
        let api = mk("api", vec![("db", None), ("init", None)], false, true);
        let eager = mk(
            "eager",
            vec![("db", Some(DependencyCondition::ServiceStarted))],
            false,
            false,
        );
        let orphan = mk("orphan", vec![("ghost", None)], false, false);
        let all = [&db, &init, &api, &eager, &orphan];
        let by_name: HashMap<&str, &CustomProcess> = all.iter().map(|p| (p.name(), *p)).collect();
        let reached = |state: CustomProcessState| Phase::Reached(state);
        let phases = |pairs: Vec<(&str, Phase)>| -> HashMap<String, Phase> {
            pairs.into_iter().map(|(n, p)| (n.to_string(), p)).collect()
        };
        let failed = || reached(CustomProcessState::Failed { reason: "x".into() });

        // nothing known yet: wait
        assert!(matches!(
            startable(&api, &by_name, &phases(vec![])),
            Startable::Later
        ));
        // db started but not ready, init done: healthy still pending
        assert!(matches!(
            startable(
                &api,
                &by_name,
                &phases(vec![
                    ("db", reached(CustomProcessState::Starting)),
                    ("init", reached(CustomProcessState::Completed))
                ])
            ),
            Startable::Later
        ));
        // both met
        assert!(matches!(
            startable(
                &api,
                &by_name,
                &phases(vec![
                    ("db", reached(CustomProcessState::Ready)),
                    ("init", reached(CustomProcessState::Completed))
                ])
            ),
            Startable::Now
        ));
        // db failed its check: never
        assert!(matches!(
            startable(
                &api,
                &by_name,
                &phases(vec![
                    ("db", failed()),
                    ("init", reached(CustomProcessState::Completed))
                ])
            ),
            Startable::Never(_)
        ));
        // init skipped: never
        assert!(matches!(
            startable(
                &api,
                &by_name,
                &phases(vec![
                    ("db", reached(CustomProcessState::Ready)),
                    ("init", Phase::Skipped)
                ])
            ),
            Startable::Never(_)
        ));
        // service_started is met by a failed check too
        assert!(matches!(
            startable(&eager, &by_name, &phases(vec![("db", failed())])),
            Startable::Now
        ));
        // an unknown dependency
        assert!(matches!(
            startable(&orphan, &by_name, &phases(vec![])),
            Startable::Never(_)
        ));
    }

    #[test]
    fn a_failure_is_an_error_unless_the_process_is_allowed_to_fail() {
        let required = CustomProcessBuilder::new()
            .with_name("api")
            .with_command("api")
            .build()
            .unwrap();
        let optional = CustomProcessBuilder::new()
            .with_name("metrics")
            .with_command("metrics")
            .with_allowed_to_fail()
            .build()
            .unwrap();

        let err = tolerate(&required, "exited with code 3".into()).unwrap_err();
        assert_eq!(err.to_string(), "Custom process 'api': exited with code 3");
        assert_eq!(
            tolerate(&optional, "exited with code 3".into()).unwrap(),
            "exited with code 3"
        );
    }
}
