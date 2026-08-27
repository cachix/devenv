use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{trace, warn};
use watchexec::{Config, WatchedPath};
use watchexec_events::{
    Tag,
    filekind::{FileEventKind, ModifyKind},
};
use watchexec_filterer_globset::GlobsetFilterer;

#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    /// The logical dependency affected by the OS event. This can differ from
    /// the subscribed path when a missing dependency is watched through its
    /// nearest existing ancestor.
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchRegistration {
    /// Existing path registered with the OS watcher.
    anchor: PathBuf,
    /// Canonical path expected in filesystem events. This differs from the
    /// logical target when the configured path passes through a symlink.
    event_target: PathBuf,
    /// Whether a missing target may use an existing ancestor as its anchor.
    watch_missing: bool,
    /// Events from the anchor must be filtered to the event target. This is
    /// true for missing targets watched through an existing ancestor.
    filtered_anchor: bool,
    /// Recursion mode for the OS anchor. Missing-file fallback anchors are
    /// always non-recursive.
    anchor_recursive: bool,
    /// Whether the logical target should be watched recursively when it exists.
    /// Fallback ancestors are always subscribed non-recursively.
    recursive: bool,
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
    /// Logical content dependencies keyed by their snapshot target.
    registrations: Arc<Mutex<HashMap<PathBuf, WatchRegistration>>>,
    operation_lock: Arc<AsyncMutex<()>>,
    config: Option<Arc<Config>>,
    recursive: bool,
}

fn direct_registration(path: &Path, recursive: bool) -> (PathBuf, WatchRegistration) {
    let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let (anchor, filtered_anchor, anchor_recursive) = existing_target_anchor(&target, recursive);
    (
        target.clone(),
        WatchRegistration {
            anchor,
            event_target: target,
            watch_missing: false,
            filtered_anchor,
            anchor_recursive,
            recursive,
        },
    )
}

fn existing_target_anchor(target: &Path, recursive: bool) -> (PathBuf, bool, bool) {
    // An inotify watch on a file follows its inode and is lost on atomic
    // replacement, while a non-recursive parent watch remains stable and
    // reports direct-child writes and renames. FSEvents does not reliably
    // report in-place child writes from a non-recursive parent, so macOS keeps
    // the exact file root and refreshes it only after a replacement event.
    if cfg!(target_os = "linux") && target.is_file() {
        (
            target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| target.to_path_buf()),
            true,
            false,
        )
    } else {
        (target.to_path_buf(), false, recursive)
    }
}

/// Resolve a logical dependency without replacing it with its watch anchor.
///
/// Canonicalising the existing prefix also makes a missing target line up with
/// paths reported through symlinked ancestors (notably /tmp on macOS).
fn logical_registration(path: &Path, recursive: bool) -> Option<(PathBuf, WatchRegistration)> {
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::env::current_dir().ok()?.join(path);
        &absolute
    };

    let Some(parent) = path.parent() else {
        let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        return Some((
            target.clone(),
            WatchRegistration {
                anchor: target.clone(),
                event_target: target.clone(),
                watch_missing: true,
                filtered_anchor: false,
                anchor_recursive: recursive,
                recursive,
            },
        ));
    };

    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor.parent()?;
    }
    let suffix = path.strip_prefix(ancestor).ok()?;
    let anchor = ancestor
        .canonicalize()
        .unwrap_or_else(|_| ancestor.to_path_buf());
    let target = anchor.join(suffix);
    let event_target = target.canonicalize().unwrap_or_else(|_| target.clone());
    let (anchor, filtered_anchor, anchor_recursive) = if target.exists() {
        existing_target_anchor(&event_target, recursive)
    } else {
        (anchor, true, false)
    };
    Some((
        target,
        WatchRegistration {
            anchor,
            event_target,
            watch_missing: true,
            filtered_anchor,
            anchor_recursive,
            recursive,
        },
    ))
}

fn subscription_paths(
    registrations: &HashMap<PathBuf, WatchRegistration>,
) -> HashMap<PathBuf, bool> {
    let mut paths = HashMap::new();
    for registration in registrations.values() {
        paths
            .entry(registration.anchor.clone())
            .and_modify(|existing| *existing |= registration.anchor_recursive)
            .or_insert(registration.anchor_recursive);
    }
    paths
}

fn set_pathset(config: &Config, paths: &HashMap<PathBuf, bool>) {
    config.pathset(paths.iter().map(|(path, recursive)| {
        if *recursive {
            WatchedPath::recursive(path)
        } else {
            WatchedPath::non_recursive(path)
        }
    }));
}

fn event_affects_target(event_path: &Path, registration: &WatchRegistration) -> bool {
    if registration.filtered_anchor {
        event_path.starts_with(&registration.event_target)
            || registration.event_target.starts_with(event_path)
    } else {
        event_path.starts_with(&registration.anchor)
    }
}

fn add_affected_targets(
    event_path: Option<&Path>,
    registrations: &HashMap<PathBuf, WatchRegistration>,
    affected: &mut HashSet<PathBuf>,
) {
    let Some(event_path) = event_path else {
        // inotify queue overflows are reported as pathless filesystem events.
        // Every logical dependency must be checked because the lost paths are
        // unknowable.
        affected.extend(registrations.keys().cloned());
        return;
    };

    let event_path = event_path
        .canonicalize()
        .unwrap_or_else(|_| event_path.to_path_buf());
    for (target, registration) in registrations {
        if event_affects_target(&event_path, registration) {
            affected.insert(target.clone());
        }
    }
}

fn add_invalidated_targets(
    event_path: Option<&Path>,
    registrations: &HashMap<PathBuf, WatchRegistration>,
    invalidated: &mut HashSet<PathBuf>,
) {
    let Some(event_path) = event_path else {
        invalidated.extend(registrations.keys().cloned());
        return;
    };

    let event_path = event_path
        .canonicalize()
        .unwrap_or_else(|_| event_path.to_path_buf());
    for (target, registration) in registrations {
        if event_path == registration.anchor {
            invalidated.insert(target.clone());
        }
    }
}

fn event_kind_may_invalidate_watch(kind: &FileEventKind) -> bool {
    matches!(
        kind,
        FileEventKind::Remove(_)
            | FileEventKind::Any
            | FileEventKind::Other
            | FileEventKind::Modify(ModifyKind::Name(_))
    )
}

fn event_may_invalidate_watch(event: &watchexec_events::Event) -> bool {
    event.tags.iter().any(|tag| match tag {
        Tag::FileEventKind(kind) => event_kind_may_invalidate_watch(kind),
        _ => false,
    })
}

impl WatcherHandle {
    /// Adds a path to watch and waits for the OS watch to be registered.
    ///
    /// Only updates the watchexec pathset when a new path is actually added.
    /// Redundant pathset updates signal the fs worker to reconcile native
    /// watches, which can disrupt platform backends.
    pub async fn watch(&self, path: &Path) {
        self.watch_registrations(std::iter::once(direct_registration(path, self.recursive)))
            .await;
    }

    /// Adds a logical dependency, using its nearest existing ancestor only as
    /// the OS subscription anchor while the dependency is missing.
    pub async fn watch_logical(&self, path: &Path) {
        self.watch_registrations(logical_registration(path, self.recursive))
            .await;
    }

    /// Adds many paths to watch in a single reconciliation and waits for the
    /// OS watches to be registered.
    ///
    /// Equivalent to calling `watch()` for each path but without the O(n^2)
    /// cost: `watch()` re-sends the entire growing pathset to watchexec per
    /// call and awaits the fs worker's ready signal each time. For large
    /// input sets (e.g. thousands of cached eval inputs) that serialisation
    /// dominates shell startup time. This method locks the pathset once,
    /// inserts all new paths, calls `config.pathset(...)` once, and awaits
    /// a single ready signal.
    pub async fn watch_many<I, P>(&self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.watch_registrations(
            paths
                .into_iter()
                .map(|path| direct_registration(path.as_ref(), self.recursive)),
        )
        .await;
    }

    /// Adds many logical dependencies in one watcher reconciliation.
    pub async fn watch_logical_many<I, P>(&self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.watch_registrations(
            paths
                .into_iter()
                .filter_map(|path| logical_registration(path.as_ref(), self.recursive)),
        )
        .await;
    }

    async fn watch_registrations<I>(&self, new_registrations: I)
    where
        I: IntoIterator<Item = (PathBuf, WatchRegistration)>,
    {
        let _op = self.operation_lock.lock().await;
        // Subscribe BEFORE updating pathset so we don't miss the ready signal.
        let mut ready = self.config.as_ref().map(|c| c.fs_ready());

        let changed = {
            let mut registrations = self.registrations.lock().unwrap();
            let before = subscription_paths(&registrations);
            for (target, mut registration) in new_registrations {
                if registrations
                    .get(&target)
                    .is_some_and(|existing| existing.watch_missing)
                {
                    // A generic cached-input watch must not downgrade a
                    // logical watch for the same path.
                    registration = logical_registration(&target, registration.recursive)
                        .map(|(_, registration)| registration)
                        .unwrap_or(registration);
                }
                registrations.insert(target, registration);
            }
            let after = subscription_paths(&registrations);
            let changed = after != before;

            if changed && let Some(ref config) = self.config {
                set_pathset(config, &after);
            }

            changed
        };

        if !changed {
            return;
        }

        if let Some(ref mut rx) = ready {
            let _ = rx.changed().await;
        }
    }

    /// Re-resolves the OS anchor for an existing logical dependency.
    ///
    /// This is intentionally separate from `watch()`: refreshing a cached
    /// input must not turn it into a missing-file watch, while logical targets
    /// need their anchor advanced when intermediate directories appear.
    pub async fn refresh(&self, target: &Path) {
        let _op = self.operation_lock.lock().await;
        let mut ready = self.config.as_ref().map(|config| config.fs_ready());

        let changed = {
            let mut registrations = self.registrations.lock().unwrap();
            let before = subscription_paths(&registrations);
            let Some(registration) = registrations.get_mut(target) else {
                return;
            };
            if !registration.watch_missing {
                return;
            }
            if let Some((_, refreshed)) = logical_registration(target, registration.recursive) {
                *registration = refreshed;
            }
            let after = subscription_paths(&registrations);
            let changed = before != after;
            if changed && let Some(ref config) = self.config {
                set_pathset(config, &after);
            }
            changed
        };

        if changed && let Some(ref mut rx) = ready {
            let _ = rx.changed().await;
        }
    }

    /// Re-register only anchors whose target may have been replaced.
    ///
    /// Native watch roots can become stale after an atomic rename. On macOS,
    /// changing any root restarts the FSEvents stream, so keeping this
    /// targeted is important.
    async fn rewatch_targets(&self, targets: &HashSet<PathBuf>) {
        let _op = self.operation_lock.lock().await;
        let Some(config) = self.config.as_ref() else {
            return;
        };

        let (without_targets, all_paths) = {
            let registrations = self.registrations.lock().unwrap();
            let all_paths = subscription_paths(&registrations);
            let mut without_targets = all_paths.clone();
            for target in targets {
                if let Some(registration) = registrations.get(target) {
                    without_targets.remove(&registration.anchor);
                }
            }
            (without_targets, all_paths)
        };

        if without_targets == all_paths {
            return;
        }

        let mut ready = config.fs_ready();
        set_pathset(config, &without_targets);
        let _ = ready.changed().await;

        let mut ready = config.fs_ready();
        set_pathset(config, &all_paths);
        let _ = ready.changed().await;
    }

    /// Force all watched paths to be re-registered with the OS.
    ///
    /// This is an explicit recovery operation. Normal file replacement uses
    /// stable parent watches on Linux and targeted re-registration on macOS,
    /// since a full FSEvents restart is disruptive.
    pub async fn rewatch_all(&self) {
        let _op = self.operation_lock.lock().await;
        let mut ready = self.config.as_ref().map(|c| c.fs_ready());

        {
            let registrations = self.registrations.lock().unwrap();
            if registrations.is_empty() {
                return;
            }

            if let Some(ref config) = self.config {
                // Clear forces the fs worker to unwatch everything
                config.pathset(std::iter::empty::<WatchedPath>());
            }
        }

        // Wait for the clear to be processed
        if let Some(ref mut rx) = ready {
            let _ = rx.changed().await;
        }

        // Now re-subscribe for the re-add
        let mut ready = self.config.as_ref().map(|c| c.fs_ready());

        {
            let mut registrations = self.registrations.lock().unwrap();
            for (target, registration) in registrations.iter_mut() {
                if registration.watch_missing
                    && let Some((_, refreshed)) =
                        logical_registration(target, registration.recursive)
                {
                    *registration = refreshed;
                }
            }
            let paths = subscription_paths(&registrations);
            if let Some(ref config) = self.config {
                set_pathset(config, &paths);
            }
        }

        if let Some(ref mut rx) = ready {
            let _ = rx.changed().await;
        }
    }

    /// Returns logical content dependencies, never their fallback OS anchors.
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.registrations.lock().unwrap().keys().cloned().collect()
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
    /// Infallible: when setup fails internally, `recv()` blocks forever.
    /// When paths is empty, the watcher starts idle but accepts new paths
    /// at runtime via `WatcherHandle::watch()`.
    pub async fn new(config: FileWatcherConfig<'_>, name: &str) -> Self {
        let (tx, rx) = mpsc::channel::<FileChangeEvent>(100);

        let registrations = Arc::new(Mutex::new(HashMap::new()));
        let operation_lock = Arc::new(AsyncMutex::new(()));

        macro_rules! empty_watcher {
            () => {
                return Self {
                    rx,
                    _tx: tx,
                    handle: WatcherHandle {
                        registrations,
                        operation_lock,
                        config: None,
                        recursive: config.recursive,
                    },
                    tasks: Vec::new(),
                }
            };
        }

        // Canonicalize watch paths to resolve symlinks.
        // On macOS, /tmp -> /private/tmp and /var -> /private/var;
        // FSEvents reports events using resolved paths.
        let initial_registrations: Vec<(PathBuf, WatchRegistration)> = config
            .paths
            .iter()
            .map(|path| direct_registration(path, config.recursive))
            .collect();
        let paths: Vec<PathBuf> = initial_registrations
            .iter()
            .map(|(_, registration)| registration.anchor.clone())
            .collect();

        {
            let mut watched = registrations.lock().unwrap();
            for (target, registration) in initial_registrations {
                watched.insert(target, registration);
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

        // Subscribe BEFORE setting the pathset so we can wait for the
        // fs worker to finish registering OS watches.
        let mut fs_ready = wx_config.fs_ready();

        set_pathset(
            &wx_config,
            &subscription_paths(&registrations.lock().unwrap()),
        );

        let handle = WatcherHandle {
            registrations: registrations.clone(),
            operation_lock,
            config: Some(wx_config.clone()),
            recursive: config.recursive,
        };

        let mut watch_info = format!(
            "File watcher started for {} watching {:?}",
            watch_name, paths
        );
        if !config.extensions.is_empty() {
            watch_info.push_str(&format!(" (extensions: {:?})", config.extensions));
        }
        trace!("{}", watch_info);

        // We use watchexec's fs::worker directly instead of Watchexec::main()
        // to avoid spawning its signal, keyboard, action, and error workers
        // that we don't need.
        let (ev_s, ev_r) = async_priority_channel::bounded(4096);
        let (er_s, mut er_r) = mpsc::channel(64);

        // Task 0: drain watchexec runtime errors.
        //
        // The fs worker reports per-path watch failures (e.g. hitting the
        // inotify watch limit) as non-fatal runtime errors on this channel.
        // If nothing keeps the receiver alive, the worker's send fails and it
        // escalates to a fatal error, exiting and leaving pending watch() calls
        // waiting on `fs_ready` forever -- which looks like a hang under the TUI.
        //
        // Draining keeps the worker alive and lets us surface the failure to the
        // user through the activity system (visible in the TUI). We report only
        // the first error to avoid flooding when many watches fail at once, and
        // trace the rest for developers.
        let err_name = watch_name.clone();
        let error_task = tokio::spawn(async move {
            let mut reported = false;
            while let Some(e) = er_r.recv().await {
                if !reported {
                    reported = true;
                    devenv_activity::message(
                        devenv_activity::ActivityLevel::Warn,
                        format!(
                            "file watcher for {err_name} failed to register a watch: {e}. \
                             hot reload may miss changes. this is often caused by reaching \
                             the inotify watch limit (raise fs.inotify.max_user_watches)."
                        ),
                    );
                }
                warn!(error = %e, watch = %err_name, "watchexec runtime error");
            }
        });

        // Task 1: fs worker — watches files via notify, sends raw events.
        let fs_config = wx_config.clone();
        let fs_errors = er_s;
        let fs_task = tokio::spawn(async move {
            if let Err(e) = watchexec::sources::fs::worker(fs_config, fs_errors, ev_s).await {
                warn!("fs worker for {} stopped: {}", watch_name, e);
                devenv_activity::message(
                    devenv_activity::ActivityLevel::Warn,
                    format!(
                        "file watcher for {watch_name} stopped: {e}. hot reload is no longer active."
                    ),
                );
            }
        });

        // Task 2: filter + throttle events, forward to our mpsc channel.
        // watchexec's throttle_collect does this but is not publicly exposed,
        // so we reimplement it here.
        let throttle = config.throttle;
        let watch_tx = tx.clone();
        let event_registrations = registrations;
        let event_handle = handle.clone();
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

                let mut affected = HashSet::new();
                let mut invalidated = HashSet::new();
                for (event, priority) in &batch {
                    if !filterer.check_event(event, *priority).unwrap_or(true) {
                        continue;
                    }
                    if !is_restart_worthy_event(event) {
                        continue;
                    }
                    let may_invalidate_watch = event_may_invalidate_watch(event);
                    let mut saw_path = false;
                    for (path, _) in event.paths() {
                        saw_path = true;
                        let registrations = event_registrations.lock().unwrap();
                        add_affected_targets(Some(path), &registrations, &mut affected);
                        if may_invalidate_watch {
                            add_invalidated_targets(Some(path), &registrations, &mut invalidated);
                        }
                    }
                    if !saw_path {
                        let registrations = event_registrations.lock().unwrap();
                        add_affected_targets(None, &registrations, &mut affected);
                        if may_invalidate_watch {
                            add_invalidated_targets(None, &registrations, &mut invalidated);
                        }
                    }
                }

                if !invalidated.is_empty() {
                    event_handle.rewatch_targets(&invalidated).await;
                }

                for path in affected {
                    // Use send().await instead of try_send to apply backpressure
                    // rather than silently dropping events when the channel is full.
                    if watch_tx.send(FileChangeEvent { path }).await.is_err() {
                        return;
                    }
                }
            }
        });

        // Wait for the fs worker to finish registering OS watches.
        // Without this, file changes that happen immediately after
        // construction could be missed.
        let _ = fs_ready.changed().await;

        Self {
            rx,
            _tx: tx,
            handle,
            tasks: vec![fs_task, filter_task, error_task],
        }
    }

    pub fn handle(&self) -> WatcherHandle {
        self.handle.clone()
    }

    pub async fn recv(&mut self) -> Option<FileChangeEvent> {
        self.rx.recv().await
    }

    pub fn try_recv(&mut self) -> Result<FileChangeEvent, tokio::sync::mpsc::error::TryRecvError> {
        self.rx.try_recv()
    }
}

fn is_restart_worthy_event(event: &watchexec_events::Event) -> bool {
    event.tags.iter().any(|tag| match tag {
        Tag::FileEventKind(kind) => is_restart_worthy_kind(kind),
        _ => false,
    })
}

fn is_restart_worthy_kind(kind: &FileEventKind) -> bool {
    match kind {
        FileEventKind::Create(_)
        | FileEventKind::Remove(_)
        | FileEventKind::Any
        | FileEventKind::Other => true,
        FileEventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Name(_)) => true,
        FileEventKind::Access(_) | FileEventKind::Modify(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::time::Duration;
    use tempfile::TempDir;

    const WATCH_TIMEOUT: Duration = Duration::from_secs(30);
    const NO_EVENT_TIMEOUT: Duration = Duration::from_millis(500);

    async fn assert_no_event(watcher: &mut FileWatcher, context: &str) {
        let result = tokio::time::timeout(NO_EVENT_TIMEOUT, watcher.recv()).await;
        assert!(
            result.is_err(),
            "unexpected file watcher event for {context}"
        );
    }

    async fn wait_for_path(watcher: &mut FileWatcher, expected: &Path, context: &str) {
        let deadline = tokio::time::Instant::now() + WATCH_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, watcher.recv()).await {
                Ok(Some(event)) if event.path == expected => return,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("watcher channel closed while waiting for {expected:?}"),
                Err(_) => panic!("timeout waiting for {expected:?} after {context}"),
            }
        }
    }

    #[test]
    fn test_restart_worthy_kind_filter() {
        use watchexec_events::filekind::{
            AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode,
        };

        assert!(is_restart_worthy_kind(&FileEventKind::Create(
            CreateKind::File
        )));
        assert!(is_restart_worthy_kind(&FileEventKind::Remove(
            RemoveKind::File
        )));
        assert!(is_restart_worthy_kind(&FileEventKind::Modify(
            ModifyKind::Data(DataChange::Any,)
        )));
        assert!(is_restart_worthy_kind(&FileEventKind::Modify(
            ModifyKind::Name(RenameMode::Any,)
        )));
        assert!(is_restart_worthy_kind(&FileEventKind::Modify(
            ModifyKind::Any
        )));
        assert!(is_restart_worthy_kind(&FileEventKind::Any));
        assert!(is_restart_worthy_kind(&FileEventKind::Other));

        assert!(!is_restart_worthy_kind(&FileEventKind::Access(
            AccessKind::Open(AccessMode::Read,)
        )));
        assert!(!is_restart_worthy_kind(&FileEventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any),
        )));
    }

    #[test]
    fn test_missing_logical_target_keeps_ancestor_out_of_content_dependencies() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let target = base.join("missing").join(".env");

        let (logical_target, registration) =
            logical_registration(&target, true).expect("resolve logical registration");

        assert_eq!(logical_target, target);
        assert_eq!(registration.anchor, base);
        assert!(registration.watch_missing);
        assert!(registration.filtered_anchor);
        assert!(!subscription_paths(&HashMap::from([(logical_target, registration,)]))[&base]);
    }

    #[test]
    fn test_missing_logical_target_filters_unrelated_parent_events() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let target = base.join("config").join(".env");
        let (_, registration) =
            logical_registration(&target, false).expect("resolve logical registration");

        assert!(!event_affects_target(
            &base.join("target/debug/deps/example.rlib"),
            &registration,
        ));
        assert!(event_affects_target(&base.join("config"), &registration,));
        assert!(event_affects_target(&target, &registration));
    }

    #[test]
    fn test_pathless_filesystem_event_affects_every_logical_target() {
        let first = PathBuf::from("/tmp/first");
        let second = PathBuf::from("/tmp/second");
        let registrations = HashMap::from([
            direct_registration(&first, false),
            direct_registration(&second, false),
        ]);
        let mut affected = HashSet::new();

        add_affected_targets(None, &registrations, &mut affected);

        assert_eq!(affected, HashSet::from([first, second]));
    }

    #[test]
    fn test_descendant_rename_does_not_invalidate_directory_anchor() {
        let directory = PathBuf::from("/tmp/project");
        let file = directory.join("devenv.nix");
        let registrations = HashMap::from([direct_registration(&directory, true)]);
        let mut invalidated = HashSet::new();

        add_invalidated_targets(Some(&file), &registrations, &mut invalidated);

        assert!(invalidated.is_empty());
    }

    #[test]
    fn test_exact_file_event_invalidates_file_anchor() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let file = temp_dir.path().join("devenv.nix");
        fs::write(&file, "{}\n").expect("create watched file");
        let file = file.canonicalize().expect("canonicalize watched file");
        let registrations = HashMap::from([direct_registration(&file, false)]);
        let mut invalidated = HashSet::new();

        add_invalidated_targets(Some(&file), &registrations, &mut invalidated);

        if cfg!(target_os = "linux") {
            assert!(invalidated.is_empty());
        } else {
            assert_eq!(invalidated, HashSet::from([file]));
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_missing_logical_target_normalizes_symlinked_ancestor() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let real = base.join("real");
        let link = base.join("link");
        fs::create_dir(&real).expect("create real directory");
        symlink(&real, &link).expect("create directory symlink");

        let (target, registration) =
            logical_registration(&link.join(".env"), false).expect("resolve logical registration");

        assert_eq!(target, real.join(".env"));
        assert_eq!(registration.anchor, real);
        assert!(registration.filtered_anchor);
    }

    #[tokio::test]
    async fn test_logical_watch_replaces_parent_anchor_when_file_appears() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let target_dir = base.join("config");
        let target = target_dir.join(".env");
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &[],
                recursive: false,
                ..Default::default()
            },
            "test-logical-watch",
        )
        .await;
        let handle = watcher.handle();

        handle.watch_logical(&target).await;
        assert_eq!(handle.watched_paths(), vec![target.clone()]);
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .get(&target)
                .expect("logical registration")
                .anchor,
            base,
        );

        // The eval cache can independently register the same path. Its direct
        // watch must not discard the dotenv watch's missing-file semantics.
        handle.watch(&target).await;
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .get(&target)
                .expect("logical registration")
                .anchor,
            base,
        );

        fs::create_dir(&target_dir).expect("create intermediate directory");
        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv())
            .await
            .expect("timeout")
            .expect("event");
        assert_eq!(event.path, target);

        handle.refresh(&target).await;
        let existing_anchor = if cfg!(target_os = "linux") {
            target_dir.clone()
        } else {
            target.clone()
        };
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .get(&target)
                .expect("logical registration")
                .anchor,
            target_dir,
        );

        File::create(&target)
            .expect("create dotenv")
            .write_all(b"VALUE=created\n")
            .expect("write dotenv");
        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv())
            .await
            .expect("timeout")
            .expect("event");
        assert_eq!(event.path, target);

        handle.refresh(&target).await;
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .get(&target)
                .expect("logical registration")
                .anchor,
            existing_anchor,
        );

        // A full rewatch retains the platform-appropriate existing-target
        // subscription and does not restore the broader fallback anchor.
        handle.rewatch_all().await;
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .get(&target)
                .expect("logical registration")
                .anchor,
            existing_anchor,
        );
    }

    #[tokio::test]
    async fn test_missing_logical_watch_ignores_unrelated_sibling() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let target = base.join(".env");
        let unrelated = base.join("unrelated.txt");
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &[],
                recursive: false,
                ..Default::default()
            },
            "test-logical-filter",
        )
        .await;

        watcher.handle().watch_logical(&target).await;
        fs::write(&unrelated, "unrelated").expect("write unrelated sibling");
        assert_no_event(&mut watcher, "unrelated sibling of missing target").await;

        fs::write(&target, "VALUE=created\n").expect("create logical target");
        wait_for_path(&mut watcher, &target, "creating logical target").await;
    }

    #[tokio::test]
    async fn test_atomic_file_replacement_keeps_watch_alive() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let target = base.join("watched.nix");
        let replacement = base.join("watched.nix.tmp");
        fs::write(&target, "same content").expect("write target");

        let paths = vec![target.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
                recursive: false,
                ..Default::default()
            },
            "test-atomic-replacement",
        )
        .await;

        // Replacing a file with identical content can be suppressed by the
        // coordinator's hash comparison, but must not leave the native watch
        // pointing at the old file.
        fs::write(&replacement, "same content").expect("write replacement");
        fs::rename(&replacement, &target).expect("replace target");
        wait_for_path(&mut watcher, &target, "atomic replacement").await;

        tokio::time::sleep(Duration::from_millis(200)).await;
        while watcher.try_recv().is_ok() {}

        fs::write(&target, "changed content").expect("modify replacement");
        wait_for_path(&mut watcher, &target, "modifying the replacement").await;
    }

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
        handle.watch(&runtime_file).await;

        File::create(&runtime_file)
            .expect("open file")
            .write_all(b"runtime modified")
            .expect("write");

        // On macOS, notify's FSEvents backend restarts the entire stream
        // when a new path is added, which replays historical events for
        // already-watched paths. Drain until we see the runtime file.
        let deadline = tokio::time::Instant::now() + WATCH_TIMEOUT;
        loop {
            let remaining = deadline - tokio::time::Instant::now();
            match tokio::time::timeout(remaining, watcher.recv()).await {
                Ok(Some(e)) if e.path == runtime_file => break,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("watcher channel closed before runtime file event"),
                Err(_) => panic!("timeout waiting for runtime file change event"),
            }
        }
    }

    #[tokio::test]
    async fn test_handle_watch_many_batches_paths() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");

        let files: Vec<PathBuf> = (0..5)
            .map(|i| {
                let p = base.join(format!("f{i}.nix"));
                File::create(&p)
                    .expect("create")
                    .write_all(b"x")
                    .expect("write");
                p
            })
            .collect();

        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &[],
                recursive: false,
                ..Default::default()
            },
            "test-watch-many",
        )
        .await;

        let handle = watcher.handle();
        handle.watch_many(files.iter()).await;

        let watched = handle.watched_paths();
        for f in &files {
            assert!(
                watched.contains(f),
                "expected {f:?} in watched set, got {watched:?}"
            );
        }

        File::create(&files[2])
            .expect("open")
            .write_all(b"changed")
            .expect("write");

        let deadline = tokio::time::Instant::now() + WATCH_TIMEOUT;
        loop {
            let remaining = deadline - tokio::time::Instant::now();
            match tokio::time::timeout(remaining, watcher.recv()).await {
                Ok(Some(e)) if e.path == files[2] => break,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("watcher channel closed"),
                Err(_) => panic!("timeout waiting for change event after watch_many"),
            }
        }
    }

    /// Reproduces the devenv hot-reload scenario:
    /// watcher starts with NO initial paths, ALL paths added via handle,
    /// then a file is modified in-place (preserving inode).
    #[tokio::test]
    async fn test_empty_watcher_with_handle_paths() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");

        let file1 = base.join("devenv.nix");
        let file2 = base.join("devenv.lock");

        File::create(&file1)
            .expect("create")
            .write_all(b"initial content")
            .expect("write");
        File::create(&file2)
            .expect("create")
            .write_all(b"lock content")
            .expect("write");

        // Create watcher with NO initial paths (like devenv reload does)
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &[],
                recursive: false,
                ..Default::default()
            },
            "test-empty",
        )
        .await;

        let handle = watcher.handle();

        // Add paths via handle (like add_watch_paths_from_cache does)
        handle.watch(&file1).await;
        handle.watch(&file2).await;

        // Modify file in-place (like swap.sh does with > redirection)
        std::fs::write(&file1, "modified content").expect("write");

        let event = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv())
            .await
            .expect("timeout waiting for event")
            .expect("event");

        assert_eq!(event.path, file1);
    }

    #[tokio::test]
    async fn test_read_only_access_does_not_emit_change_event() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let file_path = base.join("artifact.jar");

        fs::write(&file_path, b"pretend jar bytes").expect("write file");

        let paths = vec![file_path.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
                recursive: false,
                ..Default::default()
            },
            "read-only-access",
        )
        .await;

        let mut contents = Vec::new();
        File::open(&file_path)
            .expect("open file")
            .read_to_end(&mut contents)
            .expect("read file");
        assert_eq!(contents, b"pretend jar bytes");

        assert_no_event(&mut watcher, "read-only file access").await;
    }

    #[tokio::test]
    async fn test_directory_listing_does_not_emit_change_event_for_children() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let watch_dir = temp_dir.path().canonicalize().expect("canonicalize");
        let jar1 = watch_dir.join("app.jar");
        let jar2 = watch_dir.join("lib.jar");

        fs::write(&jar1, b"jar one").expect("write jar1");
        fs::write(&jar2, b"jar two").expect("write jar2");

        let paths = vec![watch_dir.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
                recursive: true,
                ..Default::default()
            },
            "directory-listing",
        )
        .await;

        let entries: Vec<_> = fs::read_dir(&watch_dir)
            .expect("read dir")
            .map(|entry| entry.expect("dir entry").file_name())
            .collect();
        assert_eq!(entries.len(), 2);

        assert_no_event(&mut watcher, "directory listing").await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_metadata_only_chmod_does_not_emit_change_event() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let file_path = base.join("artifact.jar");

        fs::write(&file_path, b"pretend jar bytes").expect("write file");

        let paths = vec![file_path.clone()];
        let mut watcher = FileWatcher::new(
            FileWatcherConfig {
                paths: &paths,
                recursive: false,
                ..Default::default()
            },
            "chmod-only",
        )
        .await;

        let mut perms = fs::metadata(&file_path).expect("metadata").permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&file_path, perms).expect("set perms 600");

        let mut perms = fs::metadata(&file_path).expect("metadata").permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&file_path, perms).expect("set perms 644");

        assert_no_event(&mut watcher, "metadata-only chmod").await;
    }
}
