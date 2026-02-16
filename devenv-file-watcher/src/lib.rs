use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use watchexec::{WatchedPath, Watchexec};
use watchexec_filterer_globset::GlobsetFilterer;

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("failed to watch path {path}: {source}")]
    Watch {
        path: PathBuf,
        source: Box<watchexec::error::RuntimeError>,
    },
}

#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
}

pub struct FileWatcherConfig {
    pub paths: Vec<PathBuf>,
    /// File extensions to watch (e.g., "rs", "js"). Empty means all.
    pub extensions: Vec<String>,
    /// Glob patterns to ignore (e.g., ".git", "*.log").
    pub ignore: Vec<String>,
    /// Watch directories recursively (default: true).
    pub recursive: bool,
    /// Throttle duration for debouncing file change events.
    /// Watchexec batches events and waits this long after the first event
    /// before triggering the action handler. Default: 100ms.
    pub throttle: Duration,
}

impl Default for FileWatcherConfig {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            extensions: Vec::new(),
            ignore: Vec::new(),
            recursive: true,
            throttle: Duration::from_millis(100),
        }
    }
}

/// Clone-able handle for runtime path addition.
///
/// Always valid -- when no watchexec task is running, `watch()` tracks paths
/// but no events fire.
#[derive(Clone)]
pub struct WatcherHandle {
    watched_paths: Arc<Mutex<HashSet<PathBuf>>>,
    wx: Option<Arc<Watchexec>>,
}

impl WatcherHandle {
    /// Adds a path to watch (non-recursive, for individual files from builders).
    pub fn watch(&self, path: &Path) -> Result<(), WatcherError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let mut paths = self.watched_paths.lock().unwrap();
        paths.insert(canonical);

        if let Some(ref wx) = self.wx {
            let parents: HashSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
            wx.config
                .pathset(parents.into_iter().map(WatchedPath::non_recursive));
        }

        Ok(())
    }

    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.lock().unwrap().iter().cloned().collect()
    }
}

/// Unified file watcher built on watchexec.
///
/// Combines watchexec's filtering with runtime path addition and path reporting.
pub struct FileWatcher {
    rx: mpsc::Receiver<FileChangeEvent>,
    // Kept alive so rx.recv() blocks (instead of returning None)
    // when no watcher task is running.
    _tx: mpsc::Sender<FileChangeEvent>,
    handle: WatcherHandle,
    task: Option<JoinHandle<()>>,
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl FileWatcher {
    /// Create a new file watcher.
    ///
    /// Infallible: when paths is empty or watchexec fails internally,
    /// `recv()` blocks forever.
    pub async fn new(config: FileWatcherConfig, name: &str) -> Self {
        let (tx, rx) = mpsc::channel::<FileChangeEvent>(100);

        let watched_paths = Arc::new(Mutex::new(HashSet::new()));

        if config.paths.is_empty() {
            return Self {
                rx,
                _tx: tx,
                handle: WatcherHandle {
                    watched_paths,
                    wx: None,
                },
                task: None,
            };
        }

        // Canonicalize watch paths to resolve symlinks.
        // On macOS, /tmp -> /private/tmp and /var -> /private/var;
        // FSEvents reports events using resolved paths.
        let paths: Vec<PathBuf> = config
            .paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            .collect();

        {
            let mut wp = watched_paths.lock().unwrap();
            for p in &paths {
                wp.insert(p.clone());
            }
        }

        let watch_name = name.to_owned();
        let watch_tx = tx.clone();

        let wx = match Watchexec::new(move |action| {
            for event in action.events.iter() {
                for (path, _) in event.paths() {
                    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                    let _ = watch_tx.try_send(FileChangeEvent { path: canonical });
                }
            }
            action
        }) {
            Ok(wx) => wx,
            Err(e) => {
                warn!("Failed to create file watcher for {}: {}", name, e);
                return Self {
                    rx,
                    _tx: tx,
                    handle: WatcherHandle {
                        watched_paths,
                        wx: None,
                    },
                    task: None,
                };
            }
        };

        let handle = WatcherHandle {
            watched_paths,
            wx: Some(wx.clone()),
        };

        // Configure watchexec before spawning its event loop so the OS
        // watcher is fully set up by the time `new()` returns.
        let ignores: Vec<(String, Option<PathBuf>)> = config
            .ignore
            .iter()
            .map(|pattern| {
                let glob_pattern = if pattern.contains('/') || pattern.starts_with("**") {
                    pattern.clone()
                } else {
                    format!("**/{}", pattern)
                };
                (glob_pattern, None)
            })
            .collect();

        let origin = paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));

        let filterer = match GlobsetFilterer::new(
            &origin,
            std::iter::empty::<(String, Option<PathBuf>)>(),
            ignores,
            std::iter::empty::<PathBuf>(),
            std::iter::empty(),
            config.extensions.iter().map(OsString::from),
        )
        .await
        {
            Ok(f) => Arc::new(f),
            Err(e) => {
                warn!("Failed to create filterer for {}: {}", watch_name, e);
                return Self {
                    rx,
                    _tx: tx,
                    handle,
                    task: None,
                };
            }
        };

        if config.recursive {
            wx.config.pathset(paths.iter().map(|p| p.as_path()));
        } else {
            // For non-recursive mode (individual files), watch parent
            // directories. FSEvents on macOS operates at directory
            // granularity and cannot watch individual files.
            let parents: HashSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
            wx.config
                .pathset(parents.into_iter().map(WatchedPath::non_recursive));
        }
        wx.config.filterer(filterer);
        wx.config.throttle(config.throttle);

        let mut watch_info = format!(
            "File watcher started for {} watching {:?}",
            watch_name, paths
        );
        if !config.extensions.is_empty() {
            watch_info.push_str(&format!(" (extensions: {:?})", config.extensions));
        }
        info!("{}", watch_info);

        // Only spawn the long-running event loop.
        let task_wx = wx.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = task_wx.main().await {
                warn!("File watcher for {} stopped: {}", watch_name, e);
            }
        });

        // Yield so the spawned event loop gets polled and starts the OS
        // watcher.  Without this, on a single-threaded runtime the spawn
        // won't run until the caller's next await point, and early file
        // changes would be missed.
        tokio::task::yield_now().await;

        Self {
            rx,
            _tx: tx,
            handle,
            task: Some(task),
        }
    }

    pub fn handle(&self) -> WatcherHandle {
        self.handle.clone()
    }

    pub async fn recv(&mut self) -> Option<FileChangeEvent> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::TempDir;

    const WATCH_TIMEOUT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn test_detects_file_modification() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let file_path = base.join("test.nix");

        File::create(&file_path)
            .expect("create file")
            .write_all(b"initial content")
            .expect("write");

        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: vec![file_path.clone()],
                recursive: false,
                ..Default::default()
            },
            "test",
        )
        .await;

        File::create(&file_path)
            .expect("open file")
            .write_all(b"modified content")
            .expect("write");

        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv()).await;
        match event {
            Ok(Some(e)) => assert_eq!(e.path, file_path),
            Ok(None) => panic!("watcher channel closed"),
            Err(_) => panic!("timeout waiting for file change event"),
        }
    }

    #[tokio::test]
    async fn test_multiple_files() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let file1 = base.join("file1.nix");
        let file2 = base.join("file2.nix");

        File::create(&file1)
            .expect("create")
            .write_all(b"1")
            .expect("write");
        File::create(&file2)
            .expect("create")
            .write_all(b"2")
            .expect("write");

        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: vec![file1.clone(), file2.clone()],
                recursive: false,
                ..Default::default()
            },
            "test",
        )
        .await;

        File::create(&file1)
            .expect("open")
            .write_all(b"1 modified")
            .expect("write");

        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv())
            .await
            .expect("timeout")
            .expect("event");

        assert!(event.path == file1 || event.path == file2);
    }

    #[tokio::test]
    async fn test_nonexistent_path_blocks_forever() {
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: vec![PathBuf::from("/this/path/does/not/exist/file.nix")],
                recursive: false,
                ..Default::default()
            },
            "test",
        )
        .await;

        let result = tokio::time::timeout(Duration::from_millis(200), watcher.recv()).await;
        assert!(
            result.is_err(),
            "recv should block (timeout) for nonexistent paths"
        );
    }

    #[tokio::test]
    async fn test_rapid_modifications() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let file_path = base.join("rapid.nix");

        File::create(&file_path)
            .expect("create")
            .write_all(b"0")
            .expect("write");

        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: vec![file_path.clone()],
                recursive: false,
                ..Default::default()
            },
            "test",
        )
        .await;

        for i in 1..=5 {
            File::create(&file_path)
                .expect("open")
                .write_all(format!("{}", i).as_bytes())
                .expect("write");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv()).await;
        assert!(event.is_ok());
    }

    #[tokio::test]
    async fn test_drops_cleanly() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let file_path = base.join("drop_test.nix");

        File::create(&file_path)
            .expect("create")
            .write_all(b"test")
            .expect("write");

        {
            let _watcher = FileWatcher::new(
                FileWatcherConfig {
                    paths: vec![file_path.clone()],
                    recursive: false,
                    ..Default::default()
                },
                "test",
            )
            .await;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_detects_file_creation_in_watched_dir() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let watch_dir = temp_dir.path().canonicalize().expect("canonicalize");

        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: vec![watch_dir.clone()],
                recursive: true,
                ..Default::default()
            },
            "test",
        )
        .await;

        let new_file = watch_dir.join("new_file.nix");
        File::create(&new_file)
            .expect("create file")
            .write_all(b"new content")
            .expect("write");

        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv()).await;
        assert!(event.is_ok());
    }

    #[tokio::test]
    async fn test_handle_adds_path_at_runtime() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let initial_file = base.join("initial.nix");
        let runtime_file = base.join("runtime.nix");

        File::create(&initial_file)
            .expect("create file")
            .write_all(b"initial")
            .expect("write");

        File::create(&runtime_file)
            .expect("create file")
            .write_all(b"runtime")
            .expect("write");

        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: vec![initial_file.clone()],
                recursive: false,
                ..Default::default()
            },
            "test",
        )
        .await;

        let handle = watcher.handle();
        handle.watch(&runtime_file).expect("add runtime watch");

        File::create(&runtime_file)
            .expect("open file")
            .write_all(b"runtime modified")
            .expect("write");

        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv())
            .await
            .expect("timeout")
            .expect("event");

        assert_eq!(event.path, runtime_file);
    }
}
