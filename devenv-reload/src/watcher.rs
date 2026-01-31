use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("failed to create watcher: {0}")]
    Create(#[from] notify::Error),
    #[error("failed to watch path {0}: {1}")]
    Watch(PathBuf, notify::Error),
}

/// Event emitted when a watched file changes
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
}

/// Handle for adding new watch paths at runtime
#[derive(Clone)]
pub struct WatcherHandle {
    watcher: Arc<Mutex<RecommendedWatcher>>,
    watched_paths: Arc<Mutex<HashSet<PathBuf>>>,
}

impl WatcherHandle {
    /// Add a new path to watch
    pub fn watch(&self, path: &Path) -> Result<(), WatcherError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Track this path for filtering
        self.watched_paths.lock().unwrap().insert(canonical);

        // Tell notify to watch it
        let mut watcher = self.watcher.lock().unwrap();
        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(|e| WatcherError::Watch(path.to_path_buf(), e))
    }
}

/// Async file watcher with debouncing
pub struct FileWatcher {
    watcher: Arc<Mutex<RecommendedWatcher>>,
    watched_paths: Arc<Mutex<HashSet<PathBuf>>>,
    receiver: mpsc::Receiver<FileChangeEvent>,
}

impl FileWatcher {
    /// Create a new file watcher for the given paths
    pub fn new(paths: &[PathBuf]) -> Result<Self, WatcherError> {
        let (tx, rx) = mpsc::channel(100);

        // Track watched paths for filtering (inotify watches directories, not files)
        let watched_paths = Arc::new(Mutex::new(HashSet::new()));
        let watched_paths_clone = watched_paths.clone();

        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    let watched = watched_paths_clone.lock().unwrap();
                    for path in event.paths {
                        // Canonicalize to match stored paths
                        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                        if watched.contains(&canonical) {
                            let _ = tx.blocking_send(FileChangeEvent { path: canonical });
                        }
                    }
                }
            }
        })?;

        let watcher = Arc::new(Mutex::new(watcher));

        // Add initial paths
        for path in paths {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            watched_paths.lock().unwrap().insert(canonical);

            watcher
                .lock()
                .unwrap()
                .watch(path, RecursiveMode::NonRecursive)
                .map_err(|e| WatcherError::Watch(path.clone(), e))?;
        }

        Ok(Self {
            watcher,
            watched_paths,
            receiver: rx,
        })
    }

    /// Get a handle for adding paths at runtime
    pub fn handle(&self) -> WatcherHandle {
        WatcherHandle {
            watcher: self.watcher.clone(),
            watched_paths: self.watched_paths.clone(),
        }
    }

    /// Receive next file change event
    pub async fn recv(&mut self) -> Option<FileChangeEvent> {
        self.receiver.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_change_event_clone() {
        let event = FileChangeEvent {
            path: PathBuf::from("/test/file.nix"),
        };
        let cloned = event.clone();
        assert_eq!(cloned.path, event.path);
    }

    #[test]
    fn test_watcher_error_display() {
        let path = PathBuf::from("/nonexistent/path");
        let notify_err = notify::Error::path_not_found();
        let err = WatcherError::Watch(path.clone(), notify_err);
        let display = format!("{}", err);
        assert!(display.contains("nonexistent"));
    }
}
