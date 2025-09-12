use std::time::Duration;

use tokio::sync::mpsc::{self, error::SendError};
use tracing::{debug, warn};

use crate::{
    network::node::NetworkNode,
    shared::constants::{
        DEFAULT_INITIAL_NODE_MONITORING_DELAY_SECONDS, DEFAULT_NODE_MONITORING_FAILURE_THRESHOLD,
        DEFAULT_NODE_MONITORING_INTERVAL_SECONDS, DEFAULT_NODE_MONITORING_LIVENESS_TIMEOUT_SECONDS,
    },
};

struct NodeWatcher {
    receiver: mpsc::Receiver<WatcherMessage>,
    node: NetworkNode,
    is_paused: bool,
    failure_tx: mpsc::Sender<String>,
    consecutive_failures: usize,
    failure_threshold: usize,
}

#[derive(Clone)]
pub(crate) struct NodeWatcherHandle {
    sender: mpsc::Sender<WatcherMessage>,
}

pub(crate) enum WatcherMessage {
    Pause,
    Resume,
    Restart { after: Option<Duration> },
}

impl NodeWatcher {
    fn new(
        receiver: mpsc::Receiver<WatcherMessage>,
        node: NetworkNode,
        failure_tx: mpsc::Sender<String>,
    ) -> Self {
        Self {
            receiver,
            node,
            is_paused: false,
            failure_tx,
            consecutive_failures: 0,
            failure_threshold: DEFAULT_NODE_MONITORING_FAILURE_THRESHOLD,
        }
    }

    async fn run(&mut self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
            DEFAULT_NODE_MONITORING_INTERVAL_SECONDS,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // sleep for a while before watching
        tokio::time::sleep(tokio::time::Duration::from_secs(
            DEFAULT_INITIAL_NODE_MONITORING_DELAY_SECONDS,
        ))
        .await;

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                },
                _ = interval.tick() => {
                    if !self.is_paused {
                        let alive = self.node
                            .wait_until_is_up(DEFAULT_NODE_MONITORING_LIVENESS_TIMEOUT_SECONDS)
                            .await
                            .is_ok();

                        if alive {
                            self.consecutive_failures = 0;
                        } else {
                            self.consecutive_failures += 1;
                            if self.consecutive_failures >= self.failure_threshold {
                                let failure_message = format!("Node '{}' was detected as down.", self.node.name());
                                if self.failure_tx.send(failure_message).await.is_err() {
                                   warn!("Watcher for node '{}' failed to send failure report.", self.node.name());
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
        debug!("Watcher for node '{}' shutting down.", self.node.name());
    }

    async fn handle_message(&mut self, msg: WatcherMessage) {
        match msg {
            WatcherMessage::Pause => {
                if !self.is_paused {
                    debug!("⏸️ Watcher for node '{}' paused.", self.node.name());
                    self.is_paused = true;
                }
            },
            WatcherMessage::Resume => {
                if self.is_paused {
                    debug!("▶️ Watcher for node '{}' resumed.", self.node.name());
                    self.is_paused = false;
                }
            },
            WatcherMessage::Restart { after } => {
                // sleep for a while to give the node a chance to restart
                let sleep_duration = after.unwrap_or_default()
                    + Duration::from_secs(DEFAULT_INITIAL_NODE_MONITORING_DELAY_SECONDS);
                tokio::time::sleep(sleep_duration).await;
            },
        }
    }
}

impl NodeWatcherHandle {
    pub(crate) fn new(node: NetworkNode, failure_tx: mpsc::Sender<String>) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut node_watcher = NodeWatcher::new(receiver, node, failure_tx);

        tokio::spawn(async move {
            node_watcher.run().await;
        });

        Self { sender }
    }

    pub(crate) async fn pause(&self) -> Result<(), SendError<WatcherMessage>> {
        self.sender.send(WatcherMessage::Pause).await
    }

    pub(crate) async fn resume(&self) -> Result<(), SendError<WatcherMessage>> {
        self.sender.send(WatcherMessage::Resume).await
    }

    pub(crate) async fn restart(
        &self,
        after: Option<Duration>,
    ) -> Result<(), SendError<WatcherMessage>> {
        self.sender.send(WatcherMessage::Restart { after }).await
    }
}
