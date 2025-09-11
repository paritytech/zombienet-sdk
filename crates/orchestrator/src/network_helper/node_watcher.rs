use tokio::sync::mpsc::{self, error::SendError};
use tracing::{info, warn};

use crate::network::node::NetworkNode;

struct NodeWatcher {
    receiver: mpsc::Receiver<WatcherMessage>,
    node: NetworkNode,
    is_paused: bool,
    failure_tx: mpsc::Sender<String>,
}

#[derive(Clone)]
pub(crate) struct NodeWatcherHandle {
    sender: mpsc::Sender<WatcherMessage>,
}

pub(crate) enum WatcherMessage {
    Pause,
    Resume,
}

impl NodeWatcher {
    pub(crate) fn new(
        receiver: mpsc::Receiver<WatcherMessage>,
        node: NetworkNode,
        failure_tx: mpsc::Sender<String>,
    ) -> Self {
        Self {
            receiver,
            node,
            is_paused: false,
            failure_tx,
        }
    }

    async fn run(&mut self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // sleep for a while before watching
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg);
                },
                _ = interval.tick() => {
                    if !self.is_paused  && self.node.wait_until_is_up(5_u64).await.is_err() {
                        let failure_message = format!("Node '{}' was detected as down.", self.node.name());
                        if self.failure_tx.send(failure_message).await.is_err() {
                           warn!("Watcher for node '{}' failed to send failure report.", self.node.name());
                        }
                        break;
                  }
                }
            }
        }
        info!("Watcher for node '{}' shutting down.", self.node.name());
    }

    fn handle_message(&mut self, msg: WatcherMessage) {
        match msg {
            WatcherMessage::Pause => {
                if !self.is_paused {
                    info!("⏸️ Watcher for node '{}' paused.", self.node.name());
                    self.is_paused = true;
                }
            },
            WatcherMessage::Resume => {
                if self.is_paused {
                    info!("▶️ Watcher for node '{}' resumed.", self.node.name());
                    self.is_paused = false;
                }
            },
        }
    }
}

impl NodeWatcherHandle {
    pub fn new(node: NetworkNode, failure_tx: mpsc::Sender<String>) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut node_watcher = NodeWatcher::new(receiver, node, failure_tx);

        tokio::spawn(async move {
            node_watcher.run().await;
        });

        Self { sender }
    }

    pub async fn pause(&self) -> Result<(), SendError<WatcherMessage>> {
        self.sender.send(WatcherMessage::Pause).await
    }

    pub async fn resume(&self) -> Result<(), SendError<WatcherMessage>> {
        self.sender.send(WatcherMessage::Resume).await
    }
}
