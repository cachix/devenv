use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use watchexec::Watchexec;
use watchexec_filterer_globset::GlobsetFilterer;

use crate::config::WatchConfig;

/// Handle for a running file watcher.
///
/// Dropping this handle aborts the watcher task and releases OS-level
/// file system watchers (inotify/FSEvents handles).
pub struct FileWatcher {
    pub rx: mpsc::Receiver<()>,
    // Kept alive so that rx.recv() blocks (instead of returning None)
    // when no watcher task is running.
    _tx: mpsc::Sender<()>,
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
    /// Spawn a file watcher for the given config.
    ///
    /// When there are no paths to watch, `rx.recv()` blocks forever.
    pub fn new(config: &WatchConfig, name: &str) -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);

        if config.paths.is_empty() {
            return Self {
                rx,
                _tx: tx,
                task: None,
            };
        }

        // Canonicalize watch paths to resolve symlinks. On macOS,
        // /tmp -> /private/tmp and /var -> /private/var; FSEvents
        // reports events using resolved paths, so the watched paths
        // must match for watchexec to attach path tags to events.
        let paths: Vec<PathBuf> = config
            .paths
            .iter()
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            .collect();
        let extensions = config.extensions.clone();
        let ignore = config.ignore.clone();
        let watch_name = name.to_owned();
        let watch_tx = tx.clone();

        let task = tokio::spawn(async move {
            let ignores: Vec<(String, Option<PathBuf>)> = ignore
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
                extensions.iter().map(OsString::from),
            )
            .await
            {
                Ok(f) => Arc::new(f),
                Err(e) => {
                    warn!("Failed to create filterer for {}: {}", watch_name, e);
                    return;
                }
            };

            let wx = match Watchexec::new(move |action| {
                if action.events.iter().any(|e| e.paths().next().is_some()) {
                    let _ = watch_tx.try_send(());
                }
                action
            }) {
                Ok(wx) => wx,
                Err(e) => {
                    warn!("Failed to create file watcher for {}: {}", watch_name, e);
                    return;
                }
            };

            wx.config.pathset(paths.iter().map(|p| p.as_path()));
            wx.config.filterer(filterer);

            let mut watch_info = format!(
                "File watcher started for {} watching {:?}",
                watch_name, paths
            );
            if !extensions.is_empty() {
                watch_info.push_str(&format!(" (extensions: {:?})", extensions));
            }
            if !ignore.is_empty() {
                watch_info.push_str(&format!(" (ignoring {:?})", ignore));
            }
            info!("{}", watch_info);

            if let Err(e) = wx.main().await {
                warn!("File watcher for {} stopped: {}", watch_name, e);
            }
        });

        Self {
            rx,
            _tx: tx,
            task: Some(task),
        }
    }
}
