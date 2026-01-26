use std::{
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

/// Events from the file watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A watched file was modified.
    Modified(PathBuf),
    /// An error occurred while watching.
    Error(String),
}

/// File watcher for monitoring log file changes.
pub struct FileWatcher {
    /// The underlying notify watcher.
    watcher: RecommendedWatcher,
    /// Receiver for watch events.
    receiver: Receiver<WatchEvent>,
}

impl FileWatcher {
    pub fn new() -> Result<Self, notify::Error> {
        let (tx, rx) = channel();

        let watcher = create_watcher(tx)?;

        Ok(Self {
            watcher,
            receiver: rx,
        })
    }

    /// Watch a file for changes.
    pub fn watch(&mut self, path: &Path) -> Result<(), notify::Error> {
        self.watcher.watch(path, RecursiveMode::NonRecursive)
    }

    /// Stop watching a file.
    pub fn unwatch(&mut self, path: &Path) -> Result<(), notify::Error> {
        self.watcher.unwatch(path)
    }

    /// Try to receive a watch event.
    pub fn try_recv(&self) -> Option<WatchEvent> {
        self.receiver.try_recv().ok()
    }
}

/// Create a watcher with the given event sender.
fn create_watcher(tx: Sender<WatchEvent>) -> Result<RecommendedWatcher, notify::Error> {
    let config = Config::default()
        .with_poll_interval(Duration::from_millis(500))
        .with_compare_contents(false);

    RecommendedWatcher::new(
        move |result: Result<notify::Event, notify::Error>| match result {
            Ok(event) => {
                if event.kind.is_modify() || event.kind.is_create() {
                    for path in event.paths {
                        let _ = tx.send(WatchEvent::Modified(path));
                    }
                }
            },
            Err(e) => {
                let _ = tx.send(WatchEvent::Error(e.to_string()));
            },
        },
        config,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        fs::{self, File},
        io::Write,
        thread,
    };

    use super::*;

    #[test]
    fn test_file_watcher_creation() {
        let watcher = FileWatcher::new();
        assert!(watcher.is_ok());
    }

    #[test]
    fn test_watch_file() {
        let file_path = temp_dir().join("test_watch_file.log");
        File::create(&file_path).unwrap();

        let mut watcher = FileWatcher::new().unwrap();
        let res = watcher.watch(&file_path);
        assert!(res.is_ok());

        let _ = fs::remove_file(&file_path);
    }

    #[test]
    fn test_watch_and_modify() {
        let file_path = temp_dir().join("test_watch_and_modify.log");
        {
            let mut file = File::create(&file_path).unwrap();
            writeln!(file, "hello").unwrap();
        }

        let mut watcher = FileWatcher::new().unwrap();
        watcher.watch(&file_path).unwrap();

        thread::sleep(Duration::from_millis(100));

        {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .unwrap();
            writeln!(file, "word").unwrap();
        }

        thread::sleep(Duration::from_millis(600));

        // Check for events.
        let event = watcher.try_recv();
        if let Some(WatchEvent::Modified(path)) = event {
            assert_eq!(path.file_name(), file_path.file_name());
        }

        let _ = fs::remove_file(&file_path);
    }
}
