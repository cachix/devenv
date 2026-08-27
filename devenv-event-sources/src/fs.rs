use std::collections::{HashMap, HashSet, VecDeque};
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

#[derive(Debug, Clone)]
pub struct FileChangeBatch {
    /// Logical dependencies affected by one throttled OS-event batch.
    pub paths: Vec<PathBuf>,
    /// The native watcher lost path information and every dependency must be
    /// checked. Keeping this as one event avoids flooding the bounded channel.
    pub rescan: bool,
}

type RegistrationId = usize;
type SharedPath = Arc<Path>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchRegistration {
    /// Existing path registered with the OS watcher.
    anchor: SharedPath,
    /// Canonical path expected in filesystem events. This differs from the
    /// logical target when the configured path passes through a symlink.
    event_target: SharedPath,
    /// Whether a missing target may use an existing ancestor as its anchor.
    watch_missing: bool,
    /// Events from the anchor must be filtered to the event target. This is
    /// true for missing targets watched through an existing ancestor.
    filtered_anchor: bool,
    /// The logical target does not exist yet. These registrations require a
    /// prefix check when an intermediate path appears; existing files use the
    /// exact-target index instead.
    pending: bool,
    /// Recursion mode for the OS anchor. Missing-file fallback anchors are
    /// always non-recursive.
    anchor_recursive: bool,
    /// Whether the logical target should be watched recursively when it exists.
    /// Fallback ancestors are always subscribed non-recursively.
    recursive: bool,
}

#[derive(Debug)]
struct RegistrationRecord {
    target: SharedPath,
    registration: WatchRegistration,
}

#[derive(Debug, Default)]
struct SpecialAnchorIndex {
    /// Registrations whose logical target differs from an unfiltered anchor.
    unfiltered: Vec<RegistrationId>,
    /// Existing aliases whose logical and event paths differ.
    exact: HashMap<SharedPath, Vec<RegistrationId>>,
    /// Missing logical targets. This is expected to stay tiny (normally dotenv
    /// paths), so prefix checks are isolated here instead of scanning all eval
    /// inputs.
    pending: Vec<RegistrationId>,
}

#[derive(Debug, Default)]
struct AnchorUsage {
    registrations: u32,
    recursive_count: u32,
}

impl AnchorUsage {
    fn recursive(&self) -> bool {
        self.recursive_count != 0
    }
}

#[derive(Debug, Default)]
struct RegistrationState {
    /// Maps logical paths to stable IDs. The path allocation is shared with the
    /// corresponding record and all secondary indexes.
    target_ids: HashMap<SharedPath, RegistrationId>,
    records: Vec<RegistrationRecord>,
    /// Anchor counters for native anchors equal to a logical target, indexed by
    /// the same stable ID as `records`. Keeping this dense preserves the small
    /// primary HashMap value used by event routing.
    target_anchor_usage: Vec<AnchorUsage>,
    /// Native anchors which are not themselves logical targets. On Linux this
    /// normally contains shared parent directories; missing paths also use an
    /// existing ancestor here. Anchors equal to a logical target are stored in
    /// `target_anchor_usage` instead of duplicating the hash key.
    extra_anchors: HashMap<SharedPath, AnchorUsage>,
    /// Allocated only when target lookup alone cannot route an event (missing
    /// paths and aliases). Normal files and directories need no secondary
    /// per-anchor allocation.
    special_anchors: HashMap<SharedPath, SpecialAnchorIndex>,
}

fn remove_id(ids: &mut Vec<RegistrationId>, id: RegistrationId) {
    if let Some(index) = ids.iter().position(|candidate| *candidate == id) {
        ids.swap_remove(index);
    }
}

impl RegistrationState {
    fn registration(&self, target: &Path) -> Option<&WatchRegistration> {
        self.target_ids
            .get(target)
            .and_then(|id| self.records.get(*id))
            .map(|record| &record.registration)
    }

    fn anchor_mode(&self, anchor: &Path) -> Option<bool> {
        self.target_ids
            .get(anchor)
            .and_then(|id| {
                let usage = &self.target_anchor_usage[*id];
                (usage.registrations != 0).then_some(usage)
            })
            .or_else(|| self.extra_anchors.get(anchor))
            .map(AnchorUsage::recursive)
    }

    fn anchors(&self) -> impl Iterator<Item = (&SharedPath, &AnchorUsage)> {
        self.target_ids
            .iter()
            .filter_map(|(path, id)| {
                let usage = &self.target_anchor_usage[*id];
                (usage.registrations != 0).then_some((path, usage))
            })
            .chain(self.extra_anchors.iter())
    }

    #[cfg(test)]
    fn anchor_count(&self) -> usize {
        self.target_ids
            .values()
            .filter(|id| self.target_anchor_usage[**id].registrations != 0)
            .count()
            + self.extra_anchors.len()
    }

    fn has_anchor(&self, anchor: &Path) -> bool {
        self.anchor_mode(anchor).is_some()
    }

    fn interned_path(&self, path: &Path) -> Option<&SharedPath> {
        self.target_ids
            .get_key_value(path)
            .map(|(path, _)| path)
            .or_else(|| self.extra_anchors.get_key_value(path).map(|(path, _)| path))
    }

    fn add_anchor_usage(&mut self, anchor: SharedPath, recursive: bool) {
        let usage = if let Some(id) = self.target_ids.get(anchor.as_ref()) {
            &mut self.target_anchor_usage[*id]
        } else {
            self.extra_anchors.entry(anchor).or_default()
        };
        usage.registrations += 1;
        if recursive {
            usage.recursive_count += 1;
        }
    }

    fn remove_anchor_usage(&mut self, anchor: &Path, recursive: bool) {
        if let Some(id) = self.target_ids.get(anchor) {
            let usage = &mut self.target_anchor_usage[*id];
            usage.registrations -= 1;
            if recursive {
                usage.recursive_count -= 1;
            }
            return;
        }

        let remove = self.extra_anchors.get_mut(anchor).is_some_and(|usage| {
            usage.registrations -= 1;
            if recursive {
                usage.recursive_count -= 1;
            }
            usage.registrations == 0
        });
        if remove {
            self.extra_anchors.remove(anchor);
        }
    }

    fn directly_indexed(target: &Path, registration: &WatchRegistration) -> bool {
        if registration.pending {
            false
        } else if registration.filtered_anchor {
            target == registration.event_target.as_ref()
        } else {
            target == registration.anchor.as_ref()
        }
    }

    fn add_to_indexes(&mut self, id: RegistrationId) {
        let (anchor, anchor_recursive, directly_indexed) = {
            let record = &self.records[id];
            (
                record.registration.anchor.clone(),
                record.registration.anchor_recursive,
                Self::directly_indexed(record.target.as_ref(), &record.registration),
            )
        };
        self.add_anchor_usage(anchor, anchor_recursive);

        if directly_indexed {
            return;
        }

        let registration = &self.records[id].registration;
        let index = self
            .special_anchors
            .entry(registration.anchor.clone())
            .or_default();
        if !registration.filtered_anchor {
            index.unfiltered.push(id);
        } else if registration.pending {
            index.pending.push(id);
        } else {
            index
                .exact
                .entry(registration.event_target.clone())
                .or_default()
                .push(id);
        }
    }

    fn remove_from_indexes(&mut self, id: RegistrationId, registration: &WatchRegistration) {
        self.remove_anchor_usage(registration.anchor.as_ref(), registration.anchor_recursive);

        let target = self.records[id].target.as_ref();
        if !Self::directly_indexed(target, registration)
            && let Some(index) = self.special_anchors.get_mut(registration.anchor.as_ref())
        {
            if !registration.filtered_anchor {
                remove_id(&mut index.unfiltered, id);
            } else if registration.pending {
                remove_id(&mut index.pending, id);
            } else if let Some(ids) = index.exact.get_mut(registration.event_target.as_ref()) {
                remove_id(ids, id);
                if ids.is_empty() {
                    index.exact.remove(registration.event_target.as_ref());
                }
            }
            if index.unfiltered.is_empty() && index.exact.is_empty() && index.pending.is_empty() {
                self.special_anchors.remove(registration.anchor.as_ref());
            }
        }
    }

    /// Insert or replace one registration. Returns whether the native pathset
    /// changed; index-only changes are immediately visible to event dispatch.
    fn insert(&mut self, target: SharedPath, mut registration: WatchRegistration) -> bool {
        let existing_id = self.target_ids.get(target.as_ref()).copied();
        let target = existing_id
            .map(|id| self.records[id].target.clone())
            .unwrap_or(target);

        // Reuse allocations for the overwhelmingly common equal target/event
        // paths and shared Linux parent anchors.
        if registration.event_target.as_ref() == target.as_ref() {
            registration.event_target = target.clone();
        }
        if registration.anchor.as_ref() == target.as_ref() {
            registration.anchor = target.clone();
        } else if registration.anchor.as_ref() == registration.event_target.as_ref() {
            registration.anchor = registration.event_target.clone();
        } else if let Some(anchor) = self.interned_path(registration.anchor.as_ref()) {
            registration.anchor = anchor.clone();
        }

        if let Some(id) = existing_id {
            if self.records[id].registration == registration {
                return false;
            }

            let old = self.records[id].registration.clone();
            let old_anchor_before = self.anchor_mode(old.anchor.as_ref());
            let new_anchor_before = self.anchor_mode(registration.anchor.as_ref());
            self.remove_from_indexes(id, &old);
            self.records[id].registration = registration.clone();
            self.add_to_indexes(id);

            old_anchor_before != self.anchor_mode(old.anchor.as_ref())
                || new_anchor_before != self.anchor_mode(registration.anchor.as_ref())
        } else {
            let anchor_before = self.anchor_mode(registration.anchor.as_ref());
            let id = self.records.len();
            let anchor_usage = self
                .extra_anchors
                .remove(target.as_ref())
                .unwrap_or_default();
            self.target_ids.insert(target.clone(), id);
            self.records.push(RegistrationRecord {
                target,
                registration: registration.clone(),
            });
            self.target_anchor_usage.push(anchor_usage);
            self.add_to_indexes(id);
            anchor_before != self.anchor_mode(registration.anchor.as_ref())
        }
    }

    fn affected_targets(&self, event_path: &Path, affected: &mut HashSet<RegistrationId>) {
        // Existing exact files and exact native roots use the primary
        // path-to-ID table directly.
        if let Some(id) = self.target_ids.get(event_path) {
            let registration = &self.records[*id].registration;
            if (registration.filtered_anchor && !registration.pending)
                || (!registration.filtered_anchor && registration.anchor.as_ref() == event_path)
            {
                affected.insert(*id);
            }
        }

        // All native anchors that can produce this event are ancestors of the
        // event path. Directory depth is small and independent of watch count.
        for (depth, ancestor) in event_path.ancestors().enumerate() {
            if depth != 0
                && let Some(id) = self.target_ids.get(ancestor)
            {
                let registration = &self.records[*id].registration;
                if !registration.filtered_anchor && registration.anchor.as_ref() == ancestor {
                    affected.insert(*id);
                }
            }

            let Some(index) = self.special_anchors.get(ancestor) else {
                continue;
            };

            affected.extend(index.unfiltered.iter().copied());
            if event_path == ancestor {
                affected.extend(index.exact.values().flatten().copied());
            } else if let Some(ids) = index.exact.get(event_path) {
                affected.extend(ids.iter().copied());
            }

            for id in &index.pending {
                let target = self.records[*id].registration.event_target.as_ref();
                if event_path.starts_with(target) || target.starts_with(event_path) {
                    affected.insert(*id);
                }
            }
        }
    }

    fn invalidated_anchor(&self, event_path: &Path) -> Option<SharedPath> {
        self.target_ids
            .get_key_value(event_path)
            .and_then(|(path, id)| {
                (self.target_anchor_usage[*id].registrations != 0).then(|| path.clone())
            })
            .or_else(|| {
                self.extra_anchors
                    .get_key_value(event_path)
                    .map(|(path, _)| path.clone())
            })
    }

    fn paths_for_ids(&self, ids: &HashSet<RegistrationId>) -> Vec<PathBuf> {
        ids.iter()
            .filter_map(|id| self.records.get(*id))
            .map(|record| record.target.to_path_buf())
            .collect()
    }

    fn targets_needing_refresh(
        &self,
        ids: &HashSet<RegistrationId>,
        may_change_watch: bool,
        refresh: &mut HashSet<RegistrationId>,
    ) {
        refresh.extend(ids.iter().copied().filter(|id| {
            self.records.get(*id).is_some_and(|record| {
                record.registration.pending
                    || (may_change_watch && record.registration.watch_missing)
            })
        }));
    }

    fn logical_targets_for_ids(&self, ids: &HashSet<RegistrationId>) -> Vec<(SharedPath, bool)> {
        ids.iter()
            .filter_map(|id| self.records.get(*id))
            .map(|record| (record.target.clone(), record.registration.recursive))
            .collect()
    }

    fn watched_paths(&self) -> Vec<PathBuf> {
        self.records
            .iter()
            .map(|record| record.target.to_path_buf())
            .collect()
    }
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
    registrations: Arc<Mutex<RegistrationState>>,
    operation_lock: Arc<AsyncMutex<()>>,
    config: Option<Arc<Config>>,
    recursive: bool,
}

fn direct_target(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn direct_registration_from_target(
    target: SharedPath,
    recursive: bool,
) -> (SharedPath, WatchRegistration) {
    let (anchor, filtered_anchor, anchor_recursive) = existing_target_anchor(&target, recursive);
    (
        target.clone(),
        WatchRegistration {
            anchor,
            event_target: target,
            watch_missing: false,
            filtered_anchor,
            pending: false,
            anchor_recursive,
            recursive,
        },
    )
}

fn direct_registration(path: &Path, recursive: bool) -> (SharedPath, WatchRegistration) {
    direct_registration_from_target(direct_target(path).into(), recursive)
}

fn existing_target_anchor(target: &SharedPath, recursive: bool) -> (SharedPath, bool, bool) {
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
                .unwrap_or_else(|| target.to_path_buf())
                .into(),
            true,
            false,
        )
    } else {
        (target.clone(), false, recursive)
    }
}

/// Resolve a logical dependency without replacing it with its watch anchor.
///
/// Canonicalising the existing prefix also makes a missing target line up with
/// paths reported through symlinked ancestors (notably /tmp on macOS).
fn logical_registration(path: &Path, recursive: bool) -> Option<(SharedPath, WatchRegistration)> {
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::env::current_dir().ok()?.join(path);
        &absolute
    };

    let Some(parent) = path.parent() else {
        let target: SharedPath = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .into();
        return Some((
            target.clone(),
            WatchRegistration {
                anchor: target.clone(),
                event_target: target.clone(),
                watch_missing: true,
                filtered_anchor: false,
                pending: false,
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
    let target_path = anchor.join(suffix);
    let (event_target_path, pending) = match target_path.canonicalize() {
        Ok(path) => (path, false),
        Err(_) => (target_path.clone(), true),
    };
    let target: SharedPath = target_path.into();
    let event_target = if event_target_path == target.as_ref() {
        target.clone()
    } else {
        event_target_path.into()
    };
    let (anchor, filtered_anchor, anchor_recursive) = if !pending {
        existing_target_anchor(&event_target, recursive)
    } else {
        (anchor.into(), true, false)
    };
    Some((
        target,
        WatchRegistration {
            anchor,
            event_target,
            watch_missing: true,
            filtered_anchor,
            pending,
            anchor_recursive,
            recursive,
        },
    ))
}

fn set_pathset<'a>(
    config: &Config,
    paths: impl IntoIterator<Item = (&'a SharedPath, &'a AnchorUsage)>,
) {
    config.pathset(paths.into_iter().map(|(path, index)| {
        if index.recursive() {
            WatchedPath::recursive(path.as_ref())
        } else {
            WatchedPath::non_recursive(path.as_ref())
        }
    }));
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
        self.watch_direct_paths(std::iter::once(path)).await;
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
        self.watch_direct_paths(paths).await;
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

    /// Resolve and insert direct registrations in bounded chunks. Holding the
    /// operation lock makes each inserted chunk visible to subsequent chunks,
    /// so duplicate targets are rejected before allocating their full
    /// registration. The registration-state lock is never held across path
    /// canonicalization or metadata syscalls, and the native pathset is still
    /// reconciled only once.
    async fn watch_direct_paths<I, P>(&self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        const RESOLUTION_BATCH_SIZE: usize = 256;

        let _op = self.operation_lock.lock().await;
        let mut ready = self.config.as_ref().map(|config| config.fs_ready());
        let mut paths = paths.into_iter();
        let mut targets = Vec::with_capacity(RESOLUTION_BATCH_SIZE);
        let mut new_targets = Vec::with_capacity(RESOLUTION_BATCH_SIZE);
        let mut registrations: Vec<(SharedPath, WatchRegistration)> =
            Vec::with_capacity(RESOLUTION_BATCH_SIZE);
        let mut changed = false;

        loop {
            targets.clear();
            for path in paths.by_ref().take(RESOLUTION_BATCH_SIZE) {
                targets.push(direct_target(path.as_ref()));
            }
            if targets.is_empty() {
                break;
            }

            new_targets.clear();
            {
                let state = self.registrations.lock().unwrap();
                new_targets.extend(targets.drain(..).filter_map(|target| {
                    (!state.target_ids.contains_key(target.as_path()))
                        .then(|| SharedPath::from(target))
                }));
            }
            if new_targets.is_empty() {
                continue;
            }

            // existing_target_anchor may perform metadata syscalls on Linux;
            // resolve those without holding the registration-state lock.
            registrations.clear();
            registrations.extend(
                new_targets
                    .drain(..)
                    .map(|target| direct_registration_from_target(target, self.recursive)),
            );
            let mut state = self.registrations.lock().unwrap();
            for (target, registration) in registrations.drain(..) {
                changed |= state.insert(target, registration);
            }
        }

        if changed && let Some(ref config) = self.config {
            let state = self.registrations.lock().unwrap();
            set_pathset(config, state.anchors());
        }

        if changed && let Some(ref mut rx) = ready {
            let _ = rx.changed().await;
        }
    }

    async fn watch_registrations<I>(&self, new_registrations: I)
    where
        I: IntoIterator<Item = (SharedPath, WatchRegistration)>,
    {
        // Registration resolution performs metadata/canonicalization syscalls;
        // finish the lazy iterator before taking either watcher lock.
        let new_registrations: Vec<_> = new_registrations.into_iter().collect();
        let _op = self.operation_lock.lock().await;
        // Subscribe BEFORE updating pathset so we don't miss the ready signal.
        let mut ready = self.config.as_ref().map(|c| c.fs_ready());

        let changed = {
            let mut state = self.registrations.lock().unwrap();
            let mut changed = false;
            for (target, mut registration) in new_registrations {
                if let Some(existing) = state.registration(target.as_ref())
                    && existing.watch_missing
                    && !registration.watch_missing
                {
                    // A generic cached-input watch must not downgrade a
                    // logical watch for the same path.
                    registration = existing.clone();
                }
                changed |= state.insert(target, registration);
            }

            if changed && let Some(ref config) = self.config {
                set_pathset(config, state.anchors());
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
        self.refresh_many(std::iter::once(target)).await;
    }

    /// Re-resolve several logical dependencies and reconcile native watches at
    /// most once. Direct watches are rejected by the ID lookup without walking
    /// or cloning the complete registration set.
    pub async fn refresh_many<I, P>(&self, targets: I)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let logical_targets: Vec<(SharedPath, bool)> = {
            let state = self.registrations.lock().unwrap();
            targets
                .into_iter()
                .filter_map(|target| {
                    let target = target.as_ref();
                    let id = *state.target_ids.get(target)?;
                    let record = &state.records[id];
                    record
                        .registration
                        .watch_missing
                        .then(|| (record.target.clone(), record.registration.recursive))
                })
                .collect()
        };

        self.refresh_logical_targets(logical_targets).await;
    }

    async fn refresh_logical_targets(&self, logical_targets: Vec<(SharedPath, bool)>) {
        if logical_targets.is_empty() {
            return;
        }

        let _op = self.operation_lock.lock().await;
        let refreshed: Vec<_> = logical_targets
            .into_iter()
            .filter_map(|(target, recursive)| logical_registration(target.as_ref(), recursive))
            .collect();
        let mut ready = self.config.as_ref().map(|config| config.fs_ready());

        let changed = {
            let mut state = self.registrations.lock().unwrap();
            let mut changed = false;
            for (target, registration) in refreshed {
                // A target could only change kind while this operation held the
                // operation lock; retain the guard for defensive correctness.
                if state
                    .registration(target.as_ref())
                    .is_some_and(|current| current.watch_missing)
                {
                    changed |= state.insert(target, registration);
                }
            }
            if changed && let Some(ref config) = self.config {
                set_pathset(config, state.anchors());
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
    async fn rewatch_anchors(&self, anchors: &HashSet<SharedPath>) {
        let _op = self.operation_lock.lock().await;
        let Some(config) = self.config.as_ref() else {
            return;
        };

        let active_anchors: HashSet<_> = {
            let state = self.registrations.lock().unwrap();
            anchors
                .iter()
                .filter(|anchor| state.has_anchor(anchor.as_ref()))
                .cloned()
                .collect()
        };
        if active_anchors.is_empty() {
            return;
        }

        let mut ready = config.fs_ready();
        {
            let state = self.registrations.lock().unwrap();
            set_pathset(
                config,
                state
                    .anchors()
                    .filter(|(path, _)| !active_anchors.contains(path.as_ref())),
            );
        }
        let _ = ready.changed().await;

        let mut ready = config.fs_ready();
        {
            let state = self.registrations.lock().unwrap();
            set_pathset(config, state.anchors());
        }
        let _ = ready.changed().await;
    }

    /// Force all watched paths to be re-registered with the OS.
    ///
    /// This is an explicit recovery operation. Normal file replacement uses
    /// stable parent watches on Linux and targeted re-registration on macOS,
    /// since a full FSEvents restart is disruptive.
    pub async fn rewatch_all(&self) {
        let _op = self.operation_lock.lock().await;
        let logical_targets: Vec<_> = {
            let state = self.registrations.lock().unwrap();
            if state.records.is_empty() {
                return;
            }
            state
                .records
                .iter()
                .filter(|record| record.registration.watch_missing)
                .map(|record| (record.target.clone(), record.registration.recursive))
                .collect()
        };
        // Resolve paths before opening the native unwatch/rewatch gap.
        let refreshed: Vec<_> = logical_targets
            .into_iter()
            .filter_map(|(target, recursive)| logical_registration(target.as_ref(), recursive))
            .collect();
        let mut ready = self.config.as_ref().map(|c| c.fs_ready());

        {
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
            let mut state = self.registrations.lock().unwrap();
            for (target, registration) in refreshed {
                state.insert(target, registration);
            }
            if let Some(ref config) = self.config {
                set_pathset(config, state.anchors());
            }
        }

        if let Some(ref mut rx) = ready {
            let _ = rx.changed().await;
        }
    }

    /// Returns logical content dependencies, never their fallback OS anchors.
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.registrations.lock().unwrap().watched_paths()
    }
}

/// Unified file watcher built on watchexec.
///
/// Uses watchexec's fs worker for file events with manual filtering and
/// throttling, without the full Watchexec event loop.
pub struct FileWatcher {
    rx: mpsc::Receiver<FileChangeBatch>,
    // Kept alive so rx.recv() blocks (instead of returning None)
    // when no watcher task is running.
    _tx: mpsc::Sender<FileChangeBatch>,
    legacy_pending: VecDeque<PathBuf>,
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
        let (tx, rx) = mpsc::channel::<FileChangeBatch>(100);

        let registrations = Arc::new(Mutex::new(RegistrationState::default()));
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
                    legacy_pending: VecDeque::new(),
                    tasks: Vec::new(),
                }
            };
        }

        // Canonicalize watch paths to resolve symlinks.
        // On macOS, /tmp -> /private/tmp and /var -> /private/var;
        // FSEvents reports events using resolved paths.
        let initial_registrations: Vec<(SharedPath, WatchRegistration)> = config
            .paths
            .iter()
            .map(|path| direct_registration(path, config.recursive))
            .collect();
        let paths: Vec<PathBuf> = initial_registrations
            .iter()
            .map(|(_, registration)| registration.anchor.to_path_buf())
            .collect();

        {
            let mut state = registrations.lock().unwrap();
            for (target, registration) in initial_registrations {
                state.insert(target, registration);
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

        set_pathset(&wx_config, registrations.lock().unwrap().anchors());

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

            let mut batch = Vec::new();
            let mut raw_event_paths = HashMap::new();
            let mut event_paths = HashMap::new();
            let mut affected = HashSet::new();
            let mut refresh = HashSet::new();
            let mut invalidated = HashSet::new();
            let mut event_affected = HashSet::new();

            loop {
                batch.clear();
                raw_event_paths.clear();
                event_paths.clear();
                affected.clear();
                refresh.clear();
                invalidated.clear();
                event_affected.clear();
                let Ok((event, priority)) = ev_r.recv().await else {
                    break;
                };
                batch.push((event, priority));

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

                let mut rescan = false;
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
                        raw_event_paths
                            .entry(path.to_path_buf())
                            .and_modify(|invalidate| *invalidate |= may_invalidate_watch)
                            .or_insert(may_invalidate_watch);
                    }
                    if !saw_path {
                        // inotify queue overflows are pathless. Preserve that
                        // information as one rescan instead of enqueueing every
                        // watched path separately.
                        rescan = true;
                    }
                }

                // Native backends often emit several event kinds for the same
                // path. Deduplicate before the comparatively expensive
                // canonicalization syscall.
                event_paths.reserve(raw_event_paths.len());
                for (path, may_invalidate_watch) in raw_event_paths.drain() {
                    let canonical = path.canonicalize().unwrap_or(path);
                    event_paths
                        .entry(canonical)
                        .and_modify(|invalidate| *invalidate |= may_invalidate_watch)
                        .or_insert(may_invalidate_watch);
                }

                let (paths, refresh_targets) = {
                    let state = event_registrations.lock().unwrap();
                    if !rescan {
                        for (path, may_invalidate_watch) in &event_paths {
                            event_affected.clear();
                            state.affected_targets(path, &mut event_affected);
                            state.targets_needing_refresh(
                                &event_affected,
                                *may_invalidate_watch,
                                &mut refresh,
                            );
                            affected.extend(event_affected.iter().copied());
                            if *may_invalidate_watch
                                && let Some(anchor) = state.invalidated_anchor(path)
                            {
                                invalidated.insert(anchor);
                            }
                        }
                    }
                    (
                        state.paths_for_ids(&affected),
                        state.logical_targets_for_ids(&refresh),
                    )
                };

                if rescan {
                    event_handle.rewatch_all().await;
                } else {
                    event_handle.refresh_logical_targets(refresh_targets).await;
                    if !invalidated.is_empty() {
                        event_handle.rewatch_anchors(&invalidated).await;
                    }
                }

                if (rescan || !paths.is_empty())
                    && watch_tx
                        .send(FileChangeBatch { paths, rescan })
                        .await
                        .is_err()
                {
                    return;
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
            legacy_pending: VecDeque::new(),
            handle,
            tasks: vec![fs_task, filter_task, error_task],
        }
    }

    pub fn handle(&self) -> WatcherHandle {
        self.handle.clone()
    }

    pub async fn recv(&mut self) -> Option<FileChangeEvent> {
        loop {
            if let Some(path) = self.legacy_pending.pop_front() {
                return Some(FileChangeEvent { path });
            }
            let batch = self.rx.recv().await?;
            self.legacy_pending.extend(if batch.rescan {
                self.handle.watched_paths()
            } else {
                batch.paths
            });
        }
    }

    pub fn try_recv(&mut self) -> Result<FileChangeEvent, tokio::sync::mpsc::error::TryRecvError> {
        loop {
            if let Some(path) = self.legacy_pending.pop_front() {
                return Ok(FileChangeEvent { path });
            }
            let batch = self.rx.try_recv()?;
            self.legacy_pending.extend(if batch.rescan {
                self.handle.watched_paths()
            } else {
                batch.paths
            });
        }
    }

    /// Receive one throttled batch without expanding rescan notifications into
    /// one channel item per watched path.
    pub async fn recv_batch(&mut self) -> Option<FileChangeBatch> {
        if !self.legacy_pending.is_empty() {
            return Some(FileChangeBatch {
                paths: self.legacy_pending.drain(..).collect(),
                rescan: false,
            });
        }
        self.rx.recv().await
    }

    pub fn try_recv_batch(
        &mut self,
    ) -> Result<FileChangeBatch, tokio::sync::mpsc::error::TryRecvError> {
        if !self.legacy_pending.is_empty() {
            return Ok(FileChangeBatch {
                paths: self.legacy_pending.drain(..).collect(),
                rescan: false,
            });
        }
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

    fn indexed_file_registration(
        target: PathBuf,
        anchor: SharedPath,
        recursive: bool,
    ) -> (SharedPath, WatchRegistration) {
        let target: SharedPath = target.into();
        (
            target.clone(),
            WatchRegistration {
                anchor,
                event_target: target,
                watch_missing: true,
                filtered_anchor: true,
                pending: false,
                anchor_recursive: recursive,
                recursive,
            },
        )
    }

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

        assert_eq!(logical_target.as_ref(), target);
        assert_eq!(registration.anchor.as_ref(), base);
        assert!(registration.watch_missing);
        assert!(registration.filtered_anchor);
        assert!(registration.pending);

        let mut state = RegistrationState::default();
        state.insert(logical_target, registration);
        assert_eq!(state.anchor_mode(base.as_path()), Some(false));
    }

    #[test]
    fn test_missing_logical_target_filters_unrelated_parent_events() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize");
        let target = base.join("config").join(".env");
        let (logical_target, registration) =
            logical_registration(&target, false).expect("resolve logical registration");
        let mut state = RegistrationState::default();
        state.insert(logical_target, registration);

        let mut affected = HashSet::new();
        state.affected_targets(&base.join("target/debug/deps/example.rlib"), &mut affected);
        assert!(affected.is_empty());
        state.affected_targets(&base.join("config"), &mut affected);
        assert_eq!(affected.len(), 1);
        affected.clear();
        state.affected_targets(&target, &mut affected);
        assert_eq!(affected.len(), 1);
    }

    #[test]
    fn test_registration_state_returns_all_logical_targets_for_rescan() {
        let first = PathBuf::from("/tmp/first");
        let second = PathBuf::from("/tmp/second");
        let mut state = RegistrationState::default();
        let (target, registration) = direct_registration(&first, false);
        state.insert(target, registration);
        let (target, registration) = direct_registration(&second, false);
        state.insert(target, registration);

        assert_eq!(
            state.watched_paths().into_iter().collect::<HashSet<_>>(),
            HashSet::from([first, second])
        );
    }

    #[test]
    fn test_large_exact_file_set_uses_compact_primary_index() {
        let anchor: SharedPath = PathBuf::from("/project/src").into();
        let mut state = RegistrationState::default();
        for index in 0..10_000 {
            let (target, registration) = indexed_file_registration(
                PathBuf::from(format!("/project/src/file-{index}.nix")),
                anchor.clone(),
                false,
            );
            state.insert(target, registration);
        }

        assert_eq!(state.records.len(), 10_000);
        assert_eq!(state.anchor_count(), 1);
        assert!(state.special_anchors.is_empty());
        let interned_anchor = state
            .interned_path(anchor.as_ref())
            .expect("anchor")
            .clone();
        assert!(
            state
                .records
                .iter()
                .all(|record| Arc::ptr_eq(&record.registration.anchor, &interned_anchor))
        );

        let expected = PathBuf::from("/project/src/file-7319.nix");
        let mut affected = HashSet::new();
        state.affected_targets(&expected, &mut affected);
        assert_eq!(state.paths_for_ids(&affected), vec![expected]);
    }

    #[test]
    fn test_anchor_subscription_modes_update_incrementally() {
        let anchor: SharedPath = PathBuf::from("/project/src").into();
        let (first, registration) =
            indexed_file_registration(PathBuf::from("/project/src/a.nix"), anchor.clone(), false);
        let (second, recursive_registration) =
            indexed_file_registration(PathBuf::from("/project/src/b.nix"), anchor.clone(), true);
        let mut state = RegistrationState::default();

        assert!(state.insert(first, registration));
        assert_eq!(state.anchor_mode(anchor.as_ref()), Some(false));
        assert!(state.insert(second, recursive_registration));
        assert_eq!(state.anchor_mode(anchor.as_ref()), Some(true));
    }

    #[test]
    fn test_exact_target_keeps_anchor_usage_in_primary_index() {
        let target: SharedPath = PathBuf::from("/project/devenv.nix").into();
        let registration = WatchRegistration {
            anchor: target.clone(),
            event_target: target.clone(),
            watch_missing: false,
            filtered_anchor: false,
            pending: false,
            anchor_recursive: false,
            recursive: false,
        };
        let mut state = RegistrationState::default();

        assert!(state.insert(target.clone(), registration));
        assert_eq!(state.anchor_count(), 1);
        assert!(state.extra_anchors.is_empty());
        assert_eq!(state.anchor_mode(target.as_ref()), Some(false));
    }

    #[test]
    fn test_new_target_absorbs_existing_extra_anchor_usage() {
        let anchor: SharedPath = PathBuf::from("/project").into();
        let (dependent, registration) =
            indexed_file_registration(PathBuf::from("/project/devenv.nix"), anchor.clone(), false);
        let mut state = RegistrationState::default();
        assert!(state.insert(dependent, registration));
        assert_eq!(state.extra_anchors.len(), 1);

        let registration = WatchRegistration {
            anchor: anchor.clone(),
            event_target: anchor.clone(),
            watch_missing: false,
            filtered_anchor: false,
            pending: false,
            anchor_recursive: false,
            recursive: false,
        };
        assert!(!state.insert(anchor.clone(), registration));

        assert!(state.extra_anchors.is_empty());
        assert_eq!(state.anchor_count(), 1);
        assert_eq!(
            state.target_anchor_usage[state.target_ids[anchor.as_ref()]].registrations,
            2
        );
    }

    #[test]
    fn test_descendant_rename_does_not_invalidate_directory_anchor() {
        let directory = PathBuf::from("/tmp/project");
        let file = directory.join("devenv.nix");
        let mut state = RegistrationState::default();
        let (target, registration) = direct_registration(&directory, true);
        state.insert(target, registration);

        assert!(state.invalidated_anchor(&file).is_none());
    }

    #[test]
    fn test_exact_file_event_invalidates_file_anchor() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let file = temp_dir.path().join("devenv.nix");
        fs::write(&file, "{}\n").expect("create watched file");
        let file = file.canonicalize().expect("canonicalize watched file");
        let mut state = RegistrationState::default();
        let (target, registration) = direct_registration(&file, false);
        state.insert(target, registration);

        if cfg!(target_os = "linux") {
            assert!(state.invalidated_anchor(&file).is_none());
        } else {
            assert_eq!(
                state.invalidated_anchor(&file).as_deref(),
                Some(file.as_path())
            );
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

        assert_eq!(target.as_ref(), real.join(".env"));
        assert_eq!(registration.anchor.as_ref(), real);
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
                .registration(&target)
                .expect("logical registration")
                .anchor
                .as_ref(),
            base.as_path(),
        );

        // The eval cache can independently register the same path. Its direct
        // watch must not discard the dotenv watch's missing-file semantics.
        handle.watch(&target).await;
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .registration(&target)
                .expect("logical registration")
                .anchor
                .as_ref(),
            base.as_path(),
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
                .registration(&target)
                .expect("logical registration")
                .anchor
                .as_ref(),
            target_dir.as_path(),
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
                .registration(&target)
                .expect("logical registration")
                .anchor
                .as_ref(),
            existing_anchor.as_path(),
        );

        // A full rewatch retains the platform-appropriate existing-target
        // subscription and does not restore the broader fallback anchor.
        handle.rewatch_all().await;
        assert_eq!(
            handle
                .registrations
                .lock()
                .unwrap()
                .registration(&target)
                .expect("logical registration")
                .anchor
                .as_ref(),
            existing_anchor.as_path(),
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

        let batch = tokio::time::timeout(WATCH_TIMEOUT, watcher.recv_batch())
            .await
            .expect("timeout")
            .expect("batch");
        assert!(!batch.rescan);
        assert_eq!(batch.paths, vec![file_path]);
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
