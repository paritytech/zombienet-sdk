//! Starting the custom processes of a network once the nodes are up: each one
//! as soon as its dependencies have met their conditions, in parallel with
//! whatever else can start, `spawn_concurrency` at a time. A process that
//! does not meet its condition, or never starts, fails the spawn.

use std::{collections::HashMap, sync::Arc, time::Duration};

use configuration::{CustomProcess, DependencyCondition};
use futures::{stream::FuturesUnordered, StreamExt};
use provider::{constants::LOCALHOST, ProviderNamespace};
use support::fs::FileSystem;
use tracing::info;

use crate::{
    errors::OrchestratorError,
    network::{node::CustomProcessState, Network},
    network_spec::NetworkSpec,
    spawner::{ports_json, prepare_process, spawn_process, PreparedProcess},
};

/// Whether a process may start, given what its dependencies reached.
enum Startable {
    Now,
    Later,
    Never(String),
}

fn startable(
    process: &CustomProcess,
    by_name: &HashMap<&str, &CustomProcess>,
    states: &HashMap<String, CustomProcessState>,
) -> Startable {
    for dep in process.depends_on() {
        let Some(target) = by_name.get(dep.name.as_str()) else {
            return Startable::Never(format!(
                "depends on '{}', which is not a custom process",
                dep.name
            ));
        };
        let condition = dep.condition_on(target);
        let Some(state) = states.get(dep.name.as_str()) else {
            return Startable::Later;
        };
        let met = match (condition, state) {
            (DependencyCondition::ServiceStarted, _) => true,
            (DependencyCondition::ServiceHealthy, CustomProcessState::Ready) => true,
            (DependencyCondition::ServiceCompletedSuccessfully, CustomProcessState::Completed) => {
                true
            },
            (_, CustomProcessState::Starting) => return Startable::Later,
            _ => false,
        };
        if !met {
            return Startable::Never(format!(
                "depends on '{}' being {}, which it is not ({state})",
                dep.name,
                condition.as_str(),
            ));
        }
    }
    Startable::Now
}

fn failed(process: &CustomProcess, reason: String) -> OrchestratorError {
    OrchestratorError::CustomProcessFailed {
        name: process.name().to_string(),
        reason,
    }
}

/// Start each process once its dependencies are met, at most
/// `spawn_concurrency` spawning at a time. The first process that fails, or
/// is never started, fails the spawn.
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
    let mut by_name: HashMap<&str, &CustomProcess> = HashMap::new();
    let mut states: HashMap<String, CustomProcessState> = HashMap::new();

    // The loader and the builder refuse a name a node or process already has,
    // but a `NetworkConfig` or `NetworkSpec` deserialized directly skips them,
    // and a process registered under a node's name would replace the node.
    // Checked before spawning, so no process runs untracked. Every process
    // parks its ports before any spawns, see `PreparedProcess`.
    let mut pending: Vec<PreparedProcess> = vec![];
    for cp in &spec.custom_processes {
        if network.has_member(cp.name()) || by_name.insert(cp.name(), cp).is_some() {
            return Err(failed(
                cp,
                "the name is already taken by a node or custom process".into(),
            ));
        }
        if let Some(file) = cp.injected_files().iter().find(|f| !f.local_path.is_file()) {
            return Err(failed(
                cp,
                format!(
                    "injected file '{}' is not a file",
                    file.local_path.display()
                ),
            ));
        }
        let prepared = prepare_process(cp, ns.as_ref())
            .map_err(|e| failed(cp, format!("could not reserve its ports: {e}")))?;
        pending.push(prepared);
    }

    // Every process's ports are known now, so a process may name another's,
    // `{{ZOMBIE:<process>:port_<name>}}`, the way it names its own. `root` is
    // where its absolute paths are: nowhere special in a container, its own
    // directory on native, where an injected file lands and it runs.
    let mut nodes_by_name = network.nodes_json()?;
    for prepared in &pending {
        let name = prepared.spec.name();
        nodes_by_name[name] = ports_json(&prepared.ports);
        nodes_by_name[name]["root"] =
            serde_json::Value::String(if ns.capabilities().prefix_with_full_path {
                ns.base_dir().join(name).to_string_lossy().into_owned()
            } else {
                String::new()
            });
    }

    // Spawning and waiting are two steps, so a `service_started` dependent
    // goes as soon as its dependency spawned, while that one is still waited
    // on. The concurrency limit is on the spawning step, as for nodes.
    let mut spawning = FuturesUnordered::new();
    let mut waiting = FuturesUnordered::new();
    loop {
        let mut i = 0;
        while i < pending.len() {
            match startable(pending[i].spec, &by_name, &states) {
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
                Startable::Never(reason) => return Err(failed(pending[i].spec, reason)),
            }
        }
        if spawning.is_empty() && waiting.is_empty() {
            break;
        }
        tokio::select! {
            Some((spec, outcome)) = spawning.next(), if !spawning.is_empty() => {
                let node = outcome.map_err(|e| failed(spec, format!("failed to spawn: {e}")))?;
                states.insert(spec.name().to_string(), CustomProcessState::Starting);
                let timeout = spec
                    .timeout()
                    .map(|secs| Duration::from_secs(u64::from(secs)))
                    .unwrap_or(default_timeout);
                waiting.push(async move {
                    let state = node.wait_ready(timeout).await;
                    (node, state)
                });
            },
            Some((mut node, state)) = waiting.next(), if !waiting.is_empty() => {
                if let CustomProcessState::Failed { reason } = state {
                    return Err(failed(node.spec(), reason));
                }
                info!("✅ {}: {state}", node.name());
                states.insert(node.name().to_string(), state.clone());
                node.state = state;
                network.add_running_custom_process(node);
            },
        }
    }
    // Only a cycle leaves something pending; the config check should have
    // caught it, so this is defensive.
    if let Some(prepared) = pending.first() {
        return Err(failed(
            prepared.spec,
            "its dependencies never started (cycle?)".into(),
        ));
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
        let mismatch = mk(
            "mismatch",
            vec![("init", Some(DependencyCondition::ServiceHealthy))],
            false,
            false,
        );
        let orphan = mk("orphan", vec![("ghost", None)], false, false);
        let all = [&db, &init, &api, &eager, &mismatch, &orphan];
        let by_name: HashMap<&str, &CustomProcess> = all.iter().map(|p| (p.name(), *p)).collect();
        let states =
            |pairs: Vec<(&str, CustomProcessState)>| -> HashMap<String, CustomProcessState> {
                pairs.into_iter().map(|(n, s)| (n.to_string(), s)).collect()
            };

        // nothing known yet: wait
        assert!(matches!(
            startable(&api, &by_name, &states(vec![])),
            Startable::Later
        ));
        // db started but not ready, init done: healthy still pending
        assert!(matches!(
            startable(
                &api,
                &by_name,
                &states(vec![
                    ("db", CustomProcessState::Starting),
                    ("init", CustomProcessState::Completed)
                ])
            ),
            Startable::Later
        ));
        // both met
        assert!(matches!(
            startable(
                &api,
                &by_name,
                &states(vec![
                    ("db", CustomProcessState::Ready),
                    ("init", CustomProcessState::Completed)
                ])
            ),
            Startable::Now
        ));
        // service_started is met as soon as the dependency spawned
        assert!(matches!(
            startable(
                &eager,
                &by_name,
                &states(vec![("db", CustomProcessState::Starting)])
            ),
            Startable::Now
        ));
        // a one-shot never becomes healthy
        assert!(matches!(
            startable(
                &mismatch,
                &by_name,
                &states(vec![("init", CustomProcessState::Completed)])
            ),
            Startable::Never(_)
        ));
        // an unknown dependency
        assert!(matches!(
            startable(&orphan, &by_name, &states(vec![])),
            Startable::Never(_)
        ));
    }

    #[test]
    fn a_failure_names_the_process_and_the_reason() {
        let api = CustomProcessBuilder::new()
            .with_name("api")
            .with_command("api")
            .build()
            .unwrap();
        assert_eq!(
            failed(&api, "exited with code 3".into()).to_string(),
            "Custom process 'api': exited with code 3"
        );
    }
}
