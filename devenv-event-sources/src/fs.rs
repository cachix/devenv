use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use watchexec::{Config, WatchedPath};
use watchexec_filterer_globset::GlobsetFilterer;

#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
}

pub struct FileWatcherConfig<'a> {
    pub paths: &'a [PathBuf],
    /// File extensions to watch (e.g., "rs", "js"). Empty means all.
    pub extensions: &'a [String],
    /// Glob patterns to ignore (e.g., ".git", "*.log").
    pub ignore: &'a [String],
    /// Watch directories recursively (default: true).
    pub recursive: bool,
    /// Throttle duration for debouncing file change events.
    /// Events are batched within this window after the first event
    /// before being delivered. Default: 100ms.
    pub throttle: Duration,
}

impl Default for FileWatcherConfig<'_> {
    fn default() -> Self {
        Self {
            paths: &[],
            extensions: &[],
            ignore: &[],
            recursive: true,
            throttle: Duration::from_millis(100),
        }
    }
}

/// Clone-able handle for runtime path addition.
///
/// Always valid -- when no watcher is running, `watch()` tracks paths
/// but no events fire.
#[derive(Clone)]
pub struct WatcherHandle {
    watched_paths: Arc<Mutex<HashSet<PathBuf>>>,
    config: Option<Arc<Config>>,
}

impl WatcherHandle {
    /// Adds a path to watch (non-recursive, for individual files from builders).
    pub fn watch(&self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let mut paths = self.watched_paths.lock().unwrap();
        paths.insert(canonical);

        if let Some(ref config) = self.config {
            let parents: HashSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
            config.pathset(parents.into_iter().map(WatchedPath::non_recursive));
        }
    }

    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.lock().unwrap().iter().cloned().collect()
    }
}

/// Unified file watcher built on watchexec.
///
/// Uses watchexec's fs worker for file events with manual filtering and
/// throttling, without the full Watchexec event loop.
pub struct FileWatcher {
    rx: mpsc::Receiver<FileChangeEvent>,
    // Kept alive so rx.recv() blocks (instead of returning None)
    // when no watcher task is running.
    _tx: mpsc::Sender<FileChangeEvent>,
    handle: WatcherHandle,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl FileWatcher {
    /// Create a new file watcher.
    ///
    /// Infallible: when paths is empty or setup fails internally,
    /// `recv()` blocks forever.
    pub async fn new(config: FileWatcherConfig<'_>, name: &str) -> Self {
        let (tx, rx) = mpsc::channel::<FileChangeEvent>(100);

        let watched_paths = Arc::new(Mutex::new(HashSet::new()));

        macro_rules! empty_watcher {
            () => {
                return Self {
                    rx,
                    _tx: tx,
                    handle: WatcherHandle {
                        watched_paths,
                        config: None,
                    },
                    tasks: Vec::new(),
                }
            };
        }

        if config.paths.is_empty() {
            empty_watcher!();
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

        // Set up the shared watchexec Config (used by fs::worker for
        // pathset changes and by WatcherHandle for runtime path addition).
        let wx_config = Arc::new(Config::default());

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
                empty_watcher!();
            }
        };

        if config.recursive {
            wx_config.pathset(paths.iter().map(|p| p.as_path()));
        } else {
            // For non-recursive mode (individual files), watch parent
            // directories. FSEvents on macOS operates at directory
            // granularity and cannot watch individual files.
            let parents: HashSet<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
            wx_config.pathset(parents.into_iter().map(WatchedPath::non_recursive));
        }

        let handle = WatcherHandle {
            watched_paths,
            config: Some(wx_config.clone()),
        };

        let mut watch_info = format!(
            "File watcher started for {} watching {:?}",
            watch_name, paths
        );
        if !config.extensions.is_empty() {
            watch_info.push_str(&format!(" (extensions: {:?})", config.extensions));
        }
        info!("{}", watch_info);

        // We use watchexec's fs::worker directly instead of Watchexec::main()
        // to avoid spawning its signal, keyboard, action, and error workers
        // that we don't need.
        let (ev_s, ev_r) = async_priority_channel::bounded(4096);
        let (er_s, _er_r) = mpsc::channel(64);

        // Task 1: fs worker — watches files via notify, sends raw events.
        let fs_config = wx_config.clone();
        let fs_errors = er_s;
        let fs_task = tokio::spawn(async move {
            if let Err(e) = watchexec::sources::fs::worker(fs_config, fs_errors, ev_s).await {
                warn!("fs worker for {} stopped: {}", watch_name, e);
            }
        });

        // Task 2: filter + throttle events, forward to our mpsc channel.
        // watchexec's throttle_collect does this but is not publicly exposed,
        // so we reimplement it here.
        let throttle = config.throttle;
        let watch_tx = tx.clone();
        let filter_task = tokio::spawn(async move {
            use watchexec::filter::Filterer;

            loop {
                let Ok((event, priority)) = ev_r.recv().await else {
                    break;
                };
                let mut batch = vec![(event, priority)];

                // Collect more events within the throttle window.
                let deadline = Instant::now() + throttle;
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match tokio::time::timeout(remaining, ev_r.recv()).await {
                        Ok(Ok(ep)) => batch.push(ep),
                        _ => break,
                    }
                }

                for (event, priority) in &batch {
                    if !filterer.check_event(event, *priority).unwrap_or(true) {
                        continue;
                    }
                    for (path, _) in event.paths() {
                        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                        let _ = watch_tx.try_send(FileChangeEvent { path: canonical });
                    }
                }
            }
        });

        // Yield so the spawned tasks get polled and the OS watcher starts.
        // Without this, on a single-threaded runtime the spawns won't run
        // until the caller's next await point, and early file changes
        // would be missed.
        tokio::task::yield_now().await;

        Self {
            rx,
            _tx: tx,
            handle,
            tasks: vec![fs_task, filter_task],
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

        let paths = vec![file_path.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
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

        let paths = vec![file1.clone(), file2.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
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
        let paths = vec![PathBuf::from("/this/path/does/not/exist/file.nix")];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
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

        let paths = vec![file_path.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
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
            let paths = vec![file_path.clone()];
            let _watcher = FileWatcher::new(
                FileWatcherConfig {
                    paths: &paths,
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

        let paths = vec![watch_dir.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
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

        let paths = vec![initial_file.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
                recursive: false,
                ..Default::default()
            },
            "test",
        )
        .await;

        let handle = watcher.handle();
        handle.watch(&runtime_file);

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
