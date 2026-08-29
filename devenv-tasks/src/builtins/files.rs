//! Native reconciliation for managed project files.
//!
//! Copy mode follows source symlinks and materializes regular files and
//! directories with their POSIX mode plus owner-write permission. Contents and
//! holes are preserved when the platform supports them. Extended attributes,
//! ACLs, resource forks, and hard-link topology are deliberately not part of
//! the portable contract.

mod destination;
mod fingerprint;
mod platform;
mod recovery;
mod state;

use crate::types::Output;
use destination::{
    Artifact, ArtifactIntent, ArtifactIntentResolution, Deferred, DeferredRemoval, Destination,
    Resolver, Staged,
};
use fd_lock::RwLock;
use fingerprint::Fingerprint;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use recovery::*;
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{self as unix_fs, Mode, OFlags},
};
use serde::{Deserialize, Serialize};
use state::StateDirectory;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use std::os::unix::fs::{MetadataExt, PermissionsExt};

const MAX_DEFERRED_CLEANUPS: usize = 4096;
const MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN: usize = 16;
const MAX_DEFERRED_CLEANUP_STEPS_PER_RECORD: usize = 16 * 1024;
const MAX_DEFERRED_CLEANUP_DURATION_PER_RUN: Duration = Duration::from_millis(200);
const MAX_ORPHAN_ARTIFACT_SCAN_ENTRIES_PER_PARENT: usize = 1024;
const MAX_ORPHAN_ARTIFACTS_PER_SWEEP: usize = MAX_DEFERRED_CLEANUPS;
const MAX_ORPHAN_ARTIFACTS_PER_RUN: usize = MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN;
const MAX_RECOVERY_LOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECOVERY_LOG_EVENTS: usize = 16 * 1024;
const MAX_RECOVERY_LOG_READ_BYTES: u64 = 16 * 1024 * 1024;

/// Test-only SIGKILL synchronization point used by the subprocess recovery
/// suite. The marker is written only after the named filesystem boundary has
/// completed, then the worker parks until its parent kills the child.
#[cfg(feature = "test-all")]
fn crash_test_failpoint(boundary: &str) {
    static CONFIG: std::sync::OnceLock<Option<(std::ffi::OsString, std::ffi::OsString)>> =
        std::sync::OnceLock::new();
    let config = CONFIG.get_or_init(|| {
        Some((
            std::env::var_os("DEVENV_FILES_TEST_CRASH_BOUNDARY")?,
            std::env::var_os("DEVENV_FILES_TEST_CRASH_MARKER")?,
        ))
    });
    let Some((configured, marker)) = config else {
        return;
    };
    if configured != std::ffi::OsStr::new(boundary) {
        return;
    }
    fs::write(marker, boundary).expect("write crash test failpoint marker");
    loop {
        thread::park();
    }
}

#[cfg(not(feature = "test-all"))]
#[inline]
fn crash_test_failpoint(_boundary: &str) {}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum CopyMode {
    Symlink,
    Seed,
    Copy,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct DesiredFile {
    path: String,
    source: PathBuf,
    mode: CopyMode,
}

#[derive(Debug, Deserialize, Serialize)]
struct FilesInputV1 {
    root: PathBuf,
    state_file: PathBuf,
    desired_digest: String,
    files: Vec<DesiredFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ManagedFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<CopyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<Fingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_fingerprint: Option<Fingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    immutable_source: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FilesState {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    desired_digest: String,
    // Retained for old shell runners, which only read this field.
    #[serde(default)]
    managed_files: Vec<String>,
    #[serde(default)]
    entries: BTreeMap<String, ManagedFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    garbage: Vec<Deferred>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    orphan_sweeps: Vec<ArtifactSweepCursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ArtifactSweepCursor {
    relative_parent: PathBuf,
    offset: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    artifacts: Vec<Deferred>,
}

#[derive(Debug, Default, Serialize)]
struct Counts {
    unchanged: usize,
    created: usize,
    updated: usize,
    removed: usize,
    conflicting: usize,
    retained: usize,
}

#[tracing::instrument(
    name = "files reconcile",
    skip(input, cancellation),
    fields(
        task_builtin.name = "files-reconcile",
        task_builtin.version = 1,
        devenv.files.unchanged,
        devenv.files.created,
        devenv.files.updated,
        devenv.files.removed,
        devenv.files.conflicting,
        devenv.files.retained,
    )
)]
pub async fn execute_v1(
    input: serde_json::Value,
    cancellation: CancellationToken,
) -> Result<Output> {
    let input: FilesInputV1 = serde_json::from_value(input)
        .into_diagnostic()
        .wrap_err("invalid files-reconcile v1 input")?;
    let worker_cancellation = cancellation.child_token();
    let cancel_on_drop = CancelOnDrop(worker_cancellation.clone());
    let counts = tokio::task::spawn_blocking(move || reconcile(input, worker_cancellation))
        .await
        .into_diagnostic()
        .wrap_err("files-reconcile worker stopped unexpectedly")??;
    drop(cancel_on_drop);

    let span = tracing::Span::current();
    span.record("devenv.files.unchanged", counts.unchanged);
    span.record("devenv.files.created", counts.created);
    span.record("devenv.files.updated", counts.updated);
    span.record("devenv.files.removed", counts.removed);
    span.record("devenv.files.conflicting", counts.conflicting);
    span.record("devenv.files.retained", counts.retained);

    tracing::info!(
        devenv.files.unchanged = counts.unchanged,
        devenv.files.created = counts.created,
        devenv.files.updated = counts.updated,
        devenv.files.removed = counts.removed,
        devenv.files.conflicting = counts.conflicting,
        devenv.files.retained = counts.retained,
        "reconciled managed files"
    );

    Ok(Output(None))
}

struct CancelOnDrop(CancellationToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn reconcile(mut input: FilesInputV1, cancellation: CancellationToken) -> Result<Counts> {
    if !input.root.is_absolute() || !input.state_file.is_absolute() {
        return Err(miette!("files-reconcile paths must be absolute"));
    }
    input.files.sort();
    reject_path_collisions(&input.files)?;
    reject_internal_path_overlaps(
        &normalize_absolute_path(&input.root),
        &normalize_absolute_path(&input.state_file),
        &input.files,
    )?;
    let mut inspected_sources = HashSet::new();
    for desired in &input.files {
        validate_relative(&desired.path)?;
        if !desired.source.is_absolute() {
            return Err(miette!(
                "managed file source must be absolute: {}",
                desired.source.display()
            ));
        }
        if desired.mode != CopyMode::Symlink && inspected_sources.insert(desired.source.as_path()) {
            fs::metadata(&desired.source)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("failed to inspect source {}", desired.source.display())
                })?;
        }
    }

    fs::create_dir_all(&input.root)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to create project root {}", input.root.display()))?;
    fs::create_dir_all(
        input
            .state_file
            .parent()
            .ok_or_else(|| miette!("files state path has no parent"))?,
    )
    .into_diagnostic()?;

    // Lexical checks above reject ordinary overlap without touching the
    // filesystem. Resolve the two roots as well so aliases cannot make a
    // managed destination overwrite its own state or lock file.
    let canonical_root = fs::canonicalize(&input.root).into_diagnostic()?;
    let state_parent = input
        .state_file
        .parent()
        .ok_or_else(|| miette!("files state path has no parent"))?;
    let canonical_state = fs::canonicalize(state_parent).into_diagnostic()?.join(
        input
            .state_file
            .file_name()
            .ok_or_else(|| miette!("files state path has no name"))?,
    );
    reject_internal_path_overlaps(&canonical_root, &canonical_state, &input.files)?;
    let original_root = std::mem::replace(&mut input.root, canonical_root);
    input.state_file = canonical_state;

    let lock_path = input.state_file.with_extension("lock");
    let recovery_path = input.state_file.with_extension("recovery");
    if lock_path == input.state_file
        || recovery_path == input.state_file
        || recovery_path == lock_path
    {
        return Err(miette!(
            "files state, lock, and recovery paths must be distinct"
        ));
    }
    let state_directory = StateDirectory::new(&input.state_file)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to open files state {}", input.state_file.display()))?;
    let lock_file = state_directory
        .open_lock()
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to open files lock {}", lock_path.display()))?;
    let mut lock = RwLock::new(lock_file);
    let guard = loop {
        if cancellation.is_cancelled() {
            return Err(miette!(
                "files reconciliation was cancelled while waiting for the lock"
            ));
        }
        match lock.try_write() {
            Ok(guard) => break guard,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("failed to lock {}", lock_path.display()));
            }
        }
    };

    let mut resolver = Resolver::new(&input.root).into_diagnostic()?;
    let mut recovery = read_recovery_anchored(&state_directory)?;
    let recovered = recover_artifacts(
        &state_directory,
        &mut resolver,
        &mut recovery,
        &cancellation,
    )?;
    // A live recovery journal proves this is a recovery path. Sweep stale
    // atomic-replace temp files here, rather than paying a directory scan on
    // ordinary unchanged reconciliation runs.
    let mut swept_state_temporaries = false;
    if recovery.initialized {
        swept_state_temporaries = true;
        if let Err(error) = state_directory.sweep_orphaned_temporaries() {
            tracing::warn!(error = %error, "could not sweep stale files state temporaries");
        }
    }
    let (existing_destinations, initially_existing) =
        reject_existing_destination_aliases(&input.root, &mut resolver, &input.files)?;

    let (mut previous, sweep_orphaned_artifacts) =
        read_state_anchored_with_sweep(&state_directory)?;
    if previous.version > 1 {
        return Err(miette!(
            "files state version {} is newer than this runner",
            previous.version
        ));
    }
    let migrated_paths = migrate_state_paths(&mut previous, &original_root, &input.root);
    let previous_orphan_sweeps = std::mem::take(&mut previous.orphan_sweeps);
    let mut orphan_sweeps = if sweep_orphaned_artifacts {
        let mut parents = BTreeSet::new();
        // A missing or malformed state cannot tell us every historic parent.
        // Limit recovery to current desired parents (plus project root), so a
        // state-loss recovery stays bounded and never walks unrelated trees.
        parents.insert(PathBuf::new());
        for desired in &input.files {
            parents.insert(
                Path::new(&desired.path)
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf(),
            );
        }
        // Legacy state still has its shell-era managed file list. Preserve
        // those parents as additional bounded recovery anchors.
        for managed in &previous.managed_files {
            parents.insert(
                Path::new(managed)
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf(),
            );
        }
        parents
            .into_iter()
            .map(|relative_parent| ArtifactSweepCursor {
                relative_parent,
                offset: 0,
                artifacts: Vec::new(),
            })
            .collect()
    } else {
        previous_orphan_sweeps.clone()
    };
    let mut remaining_sweeps = Vec::new();
    let persisted_sweep_artifacts = orphan_sweeps
        .iter()
        .map(|cursor| cursor.artifacts.len())
        .sum::<usize>();
    let mut remaining_artifact_slots = MAX_ORPHAN_ARTIFACTS_PER_RUN
        .min(MAX_ORPHAN_ARTIFACTS_PER_SWEEP.saturating_sub(persisted_sweep_artifacts));
    for mut cursor in orphan_sweeps.drain(..) {
        if remaining_artifact_slots == 0 {
            // The bounded recovery cache is full. Publish only the exact
            // identities already verified, then stop this parent rather than
            // persisting a cursor that can never reach EOF. Any later names
            // are deliberately left untouched for a future state-loss sweep.
            tracing::warn!(parent = %cursor.relative_parent.display(), "files artifact sweep reached its recovery-record limit; leaving unscanned artifacts untouched");
            if !cursor.artifacts.is_empty() {
                previous.garbage.extend(cursor.artifacts);
            }
            continue;
        }
        match resolver.discover_orphaned_artifacts(
            &cursor.relative_parent,
            cursor.offset,
            &cancellation,
            MAX_ORPHAN_ARTIFACT_SCAN_ENTRIES_PER_PARENT,
            remaining_artifact_slots,
        ) {
            Ok((orphans, complete, offset)) => {
                remaining_artifact_slots -= orphans.len();
                cursor.artifacts.extend(orphans);
                if !complete {
                    cursor.offset = offset;
                    // Candidates remain out of normal cleanup until this
                    // parent reaches EOF. Removing them earlier would shift
                    // directory positions and invalidate the numeric cursor.
                    remaining_sweeps.push(cursor);
                } else {
                    previous.garbage.extend(cursor.artifacts);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                return Err(miette!(
                    "files reconciliation was cancelled during artifact recovery"
                ));
            }
            Err(error) => {
                tracing::warn!(parent = %cursor.relative_parent.display(), error = %error, "could not sweep orphaned files artifacts after state loss");
                remaining_sweeps.push(cursor);
            }
        }
    }
    previous.orphan_sweeps = remaining_sweeps;
    let mut live_artifacts =
        publish_recovered_artifacts(&state_directory, &mut previous, &mut recovery, recovered)?;
    let digest_matches = previous.version == 1 && previous.desired_digest == input.desired_digest;
    let desired_paths: BTreeSet<_> = input.files.iter().map(|f| f.path.as_str()).collect();
    let mut counts = Counts::default();
    let managed_paths_match = previous.managed_files.len() == desired_paths.len()
        && previous
            .managed_files
            .iter()
            .all(|path| desired_paths.contains(path.as_str()));
    let entries_match_managed = previous.entries.len() == previous.managed_files.len()
        && previous
            .managed_files
            .iter()
            .all(|path| previous.entries.contains_key(path));
    let managed_paths_are_sorted = previous
        .managed_files
        .windows(2)
        .all(|pair| pair[0] < pair[1]);
    let mut state_changed = migrated_paths
        || previous.version != 1
        || previous.desired_digest != input.desired_digest
        || !entries_match_managed
        || !managed_paths_are_sorted;
    state_changed |= previous.orphan_sweeps != previous_orphan_sweeps;
    let mut retained_garbage = std::mem::take(&mut previous.garbage);
    let garbage_before_dedup = retained_garbage.len();
    let mut seen_garbage = HashSet::with_capacity(retained_garbage.len());
    retained_garbage.retain(|entry| seen_garbage.insert(entry.clone()));
    state_changed |= retained_garbage.len() != garbage_before_dedup;
    let mut previous_managed_files = std::mem::take(&mut previous.managed_files);
    previous_managed_files.sort();
    previous_managed_files.dedup();
    let mut managed = std::mem::take(&mut previous.entries);
    managed.retain(|path, _| previous_managed_files.binary_search(path).is_ok());
    for path in &previous_managed_files {
        managed.entry(path.clone()).or_insert_with(|| ManagedFile {
            source: None,
            mode: None,
            fingerprint: None,
            source_fingerprint: None,
            immutable_source: None,
        });
    }
    let orphan_sweeps = std::mem::take(&mut previous.orphan_sweeps);
    drop(previous);
    let mut errors = Vec::new();
    let cleanup_paths: &[String] = if digest_matches && managed_paths_match {
        &[]
    } else {
        &previous_managed_files
    };
    for old_path in cleanup_paths {
        if cancellation.is_cancelled() {
            errors.push("files reconciliation was cancelled".to_string());
            break;
        }
        if desired_paths.contains(old_path.as_str()) {
            continue;
        }
        match cleanup_removed(&mut resolver, old_path, managed.get(old_path)) {
            Ok(Cleanup::Removed) => {
                counts.removed += 1;
                state_changed |= managed.remove(old_path).is_some();
            }
            Ok(Cleanup::Absent) => {
                state_changed |= managed.remove(old_path).is_some();
            }
            Ok(Cleanup::Retained) => {
                counts.retained += 1;
                tracing::warn!(
                    path = %old_path,
                    "retaining managed file removed from configuration; remove it manually if it is no longer needed"
                );
                state_changed |= managed.remove(old_path).is_some();
            }
            Err(error) => {
                tracing::warn!(path = %old_path, error = %error, "failed to clean up managed file");
                errors.push(format!("failed to clean up {old_path}: {error:#}"));
            }
        }
    }

    let mut claimed_destinations = existing_destinations;
    let mut source_fingerprints = HashMap::new();
    let initial_garbage_len = retained_garbage.len();
    let mut garbage = retained_garbage;
    for desired in &input.files {
        if cancellation.is_cancelled() {
            errors.push("files reconciliation was cancelled".to_string());
            break;
        }
        let old = managed.get(&desired.path);
        if !initially_existing.contains(&desired.path)
            && let Err(error) = claim_existing_destination(
                &input.root,
                &mut resolver,
                &desired.path,
                &mut claimed_destinations,
            )
        {
            tracing::warn!(path = %desired.path, error = %error, "could not reconcile managed file");
            errors.push(format!("failed to reconcile {}: {error:#}", desired.path));
            continue;
        }
        match reconcile_one(
            &mut resolver,
            &state_directory,
            &mut recovery,
            desired,
            old,
            &mut source_fingerprints,
            &mut garbage,
            &mut live_artifacts,
            &cancellation,
        ) {
            Ok(result) => {
                if matches!(result.decision, Decision::Created | Decision::Updated)
                    && let Err(error) = claim_existing_destination(
                        &input.root,
                        &mut resolver,
                        &desired.path,
                        &mut claimed_destinations,
                    )
                {
                    tracing::warn!(path = %desired.path, error = %error, "managed path became an alias during reconciliation");
                    errors.push(format!("failed to reconcile {}: {error:#}", desired.path));
                    state_changed |= managed.remove(&desired.path).is_some();
                    continue;
                }
                match result.decision {
                    Decision::Unchanged => counts.unchanged += 1,
                    Decision::Created => counts.created += 1,
                    Decision::Updated => counts.updated += 1,
                    Decision::Retained => {
                        counts.retained += 1;
                        tracing::warn!(
                            path = %desired.path,
                            "retaining existing managed path instead of overwriting it"
                        );
                    }
                    Decision::Conflict => {
                        counts.conflicting += 1;
                        tracing::warn!(
                            path = %desired.path,
                            "conflicting managed file was not replaced"
                        );
                    }
                }
                if !matches!(result.decision, Decision::Conflict) {
                    let entry = ManagedFile {
                        source: Some(desired.source.clone()),
                        mode: Some(desired.mode.clone()),
                        fingerprint: result.fingerprint,
                        source_fingerprint: result.source_fingerprint,
                        immutable_source: result.immutable_source,
                    };
                    if managed.get(&desired.path) != Some(&entry) {
                        managed.insert(desired.path.clone(), entry);
                        state_changed = true;
                    }
                } else if !old.is_some_and(|entry| entry.mode == Some(CopyMode::Copy)) {
                    state_changed |= managed.remove(&desired.path).is_some();
                }
            }
            Err(error) => {
                tracing::warn!(path = %desired.path, error = %error, "could not reconcile managed file");
                errors.push(format!("failed to reconcile {}: {error:#}", desired.path));
            }
        }
    }

    if garbage.len() != initial_garbage_len {
        state_changed = true;
    }
    if state_changed && !swept_state_temporaries {
        // Changed runs already pay state/journal I/O, so opportunistically
        // reclaim interrupted atomic-replace temporaries without affecting
        // the unchanged fast path.
        if let Err(error) = state_directory.sweep_orphaned_temporaries() {
            tracing::warn!(error = %error, "could not sweep stale files state temporaries");
        }
    }
    let state = FilesState {
        version: 1,
        desired_digest: input.desired_digest,
        managed_files: managed.keys().cloned().collect(),
        entries: managed,
        garbage,
        orphan_sweeps,
    };
    if state_changed && let Err(error) = write_state(&state_directory, &state) {
        drop(guard);
        let cleanup = CancellationToken::new();
        let cleanup_deadline = Instant::now() + MAX_DEFERRED_CLEANUP_DURATION_PER_RUN;
        for deferred in state
            .garbage
            .iter()
            .take(MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN)
        {
            if let Some(artifact) = live_artifacts.remove(deferred) {
                let _ = artifact.remove_bounded(
                    &cleanup,
                    MAX_DEFERRED_CLEANUP_STEPS_PER_RECORD,
                    cleanup_deadline.saturating_duration_since(Instant::now()),
                );
            } else {
                let _ = resolver.remove_deferred_bounded(
                    deferred,
                    &cleanup,
                    MAX_DEFERRED_CLEANUP_STEPS_PER_RECORD,
                    cleanup_deadline.saturating_duration_since(Instant::now()),
                );
            }
        }
        return Err(error);
    }
    if state_changed
        && let Err(error) =
            forget_persisted_recovery(&state_directory, &mut recovery, &state.garbage)
    {
        // The main state now owns these exact cleanup identities. A stale
        // sidecar is safe and will be deduplicated during the next recovery.
        tracing::warn!(error = %error, "could not prune persisted files recovery records");
    }
    drop(guard);
    let cleanup_after_unlock = state
        .garbage
        .into_iter()
        .take(MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN)
        .collect::<Vec<_>>();
    let mut attempted_garbage = Vec::with_capacity(cleanup_after_unlock.len());
    let mut resolved_garbage = Vec::new();
    let cleanup_deadline = Instant::now() + MAX_DEFERRED_CLEANUP_DURATION_PER_RUN;
    for deferred in cleanup_after_unlock {
        attempted_garbage.push(deferred.clone());
        let result = if let Some(artifact) = live_artifacts.remove(&deferred) {
            artifact.remove_bounded(
                &cancellation,
                MAX_DEFERRED_CLEANUP_STEPS_PER_RECORD,
                cleanup_deadline.saturating_duration_since(Instant::now()),
            )
        } else {
            resolver.remove_deferred_bounded(
                &deferred,
                &cancellation,
                MAX_DEFERRED_CLEANUP_STEPS_PER_RECORD,
                cleanup_deadline.saturating_duration_since(Instant::now()),
            )
        };
        match result {
            Ok(DeferredRemoval::Removed) => resolved_garbage.push(deferred),
            Ok(DeferredRemoval::IdentityMismatch) => {
                tracing::warn!("could not safely find replaced file for cleanup");
                resolved_garbage.push(deferred);
            }
            Ok(DeferredRemoval::Incomplete) => {
                tracing::debug!("deferred file cleanup reached its per-run work limit");
            }
            Err(error) => {
                tracing::warn!(error = %error, "could not remove replaced file");
            }
        }
    }
    update_garbage_after_cleanup(
        &state_directory,
        &mut lock,
        &attempted_garbage,
        &resolved_garbage,
        &cancellation,
    );
    if !errors.is_empty() {
        return Err(miette!(errors.join("; ")));
    }
    Ok(counts)
}

fn update_garbage_after_cleanup(
    state_directory: &StateDirectory,
    lock: &mut RwLock<File>,
    attempted: &[Deferred],
    resolved: &[Deferred],
    cancellation: &CancellationToken,
) {
    if attempted.is_empty() || cancellation.is_cancelled() {
        return;
    }
    let guard = loop {
        if cancellation.is_cancelled() {
            return;
        }
        match lock.try_write() {
            Ok(guard) => break guard,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(error = %error, "could not lock files state to prune cleanup records");
                return;
            }
        }
    };
    let Ok(mut state) = read_state_anchored(state_directory) else {
        return;
    };
    if state.version > 1 {
        return;
    }
    let attempted: HashSet<_> = attempted.iter().collect();
    let resolved: HashSet<_> = resolved.iter().collect();
    let previous = std::mem::take(&mut state.garbage);
    let mut deferred = Vec::new();
    state.garbage.reserve(previous.len());
    for entry in &previous {
        if resolved.contains(entry) {
            continue;
        }
        if attempted.contains(entry) {
            deferred.push(entry.clone());
        } else {
            state.garbage.push(entry.clone());
        }
    }
    state.garbage.extend(deferred);
    if state.garbage != previous
        && let Err(error) = write_state(state_directory, &state)
    {
        tracing::warn!(error = %error, "could not update deferred file cleanup records");
    }
    drop(guard);
}

fn reject_path_collisions(files: &[DesiredFile]) -> Result<()> {
    let paths: BTreeSet<_> = files.iter().map(|file| file.path.as_str()).collect();
    if paths.len() != files.len() {
        for pair in files.windows(2) {
            if pair[0].path == pair[1].path {
                return Err(miette!("duplicate managed file path: {}", pair[0].path));
            }
        }
    }
    for path in &paths {
        let mut parent = Path::new(path).parent();
        while let Some(candidate) = parent {
            if let Some(candidate) = candidate.to_str()
                && paths.contains(candidate)
            {
                return Err(miette!(
                    "managed file paths overlap: {candidate} is a parent of {path}"
                ));
            }
            parent = candidate.parent();
        }
    }
    Ok(())
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn reject_internal_path_overlaps(
    root: &Path,
    state_file: &Path,
    files: &[DesiredFile],
) -> Result<()> {
    let lock_path = state_file.with_extension("lock");
    let recovery_path = state_file.with_extension("recovery");
    for desired in files {
        let destination = root.join(&desired.path);
        for internal in [state_file, &lock_path, &recovery_path] {
            if internal.starts_with(&destination) || destination.starts_with(internal) {
                return Err(miette!(
                    "managed file path overlaps internal files state: {}",
                    desired.path
                ));
            }
        }
    }
    Ok(())
}

type FileIdentity = (u64, u64);

fn destination_identity(resolver: &mut Resolver, relative: &str) -> Result<Option<FileIdentity>> {
    let destination = match resolver.destination(relative, false) {
        Ok(destination) => destination,
        // Reconciliation reports unsafe or invalid ancestors. Alias detection
        // must not turn those later per-entry errors into an early abort.
        Err(_) => return Ok(None),
    };
    Ok(destination
        .metadata()
        .into_diagnostic()?
        .map(|metadata| (metadata.device, metadata.inode)))
}

fn path_spelling_is_exact(root: &Path, relative: &str) -> Result<bool> {
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Ok(false);
        };
        let exact = fs::read_dir(&current)
            .into_diagnostic()?
            .any(|entry| entry.is_ok_and(|entry| entry.file_name() == name));
        if !exact {
            return Ok(false);
        }
        current.push(name);
    }
    Ok(true)
}

fn claim_existing_destination(
    root: &Path,
    resolver: &mut Resolver,
    relative: &str,
    claimed: &mut HashMap<FileIdentity, String>,
) -> Result<bool> {
    let Some(identity) = destination_identity(resolver, relative)? else {
        return Ok(false);
    };
    if let Some(previous) = claimed.get(&identity) {
        if previous == relative {
            return Ok(true);
        }
        if !path_spelling_is_exact(root, previous)? || !path_spelling_is_exact(root, relative)? {
            return Err(miette!(
                "filesystem-equivalent managed file paths: {previous} and {relative}"
            ));
        }
        tracing::trace!(first = %previous, second = %relative, "managed paths are hard links");
    } else {
        claimed.insert(identity, relative.to_string());
    }
    Ok(true)
}

fn reject_existing_destination_aliases(
    root: &Path,
    resolver: &mut Resolver,
    files: &[DesiredFile],
) -> Result<(HashMap<FileIdentity, String>, HashSet<String>)> {
    let mut claimed = HashMap::new();
    let mut existing = HashSet::new();
    for desired in files {
        if claim_existing_destination(root, resolver, &desired.path, &mut claimed)? {
            existing.insert(desired.path.clone());
        }
    }
    Ok((claimed, existing))
}

fn read_state_anchored(state_directory: &StateDirectory) -> Result<FilesState> {
    read_state_anchored_with_sweep(state_directory).map(|(state, _)| state)
}

/// Shell-era state used the original attribute names, including absolute paths
/// (notably Claude settings). Normalize lexically before any filesystem access:
/// resolving an untrusted old destination could follow a link outside the root.
fn migrate_state_paths(state: &mut FilesState, original_root: &Path, root: &Path) -> bool {
    fn relative(path: &str, original_root: &Path, root: &Path) -> Option<String> {
        let path = Path::new(path);
        if path.components().any(|c| c == Component::ParentDir) {
            return None;
        }
        let path = if path.is_absolute() {
            path.strip_prefix(original_root)
                .or_else(|_| path.strip_prefix(root))
                .ok()?
        } else {
            path
        };
        let normalized: PathBuf = path
            .components()
            .filter(|c| *c != Component::CurDir)
            .collect();
        let normalized = normalized.to_str()?;
        validate_relative(normalized).ok()?;
        Some(normalized.to_owned())
    }

    let mut changed = false;
    state.managed_files.retain_mut(|old_path| {
        // Current state already uses clean relative names. Keep their existing
        // allocations on the ordinary unchanged path.
        if old_path
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
        {
            return true;
        }
        let Some(path) = relative(old_path, original_root, root) else {
            tracing::warn!(path = %old_path, "forgetting unsafe legacy managed path without accessing it");
            state.entries.remove(old_path.as_str());
            changed = true;
            return false;
        };
        if *old_path != path {
            changed = true;
            // Prefer an existing canonical record if a previous upgrade wrote
            // both spellings. Never overwrite its newer ownership metadata.
            if let Some(entry) = state.entries.remove(old_path.as_str()) {
                state.entries.entry(path.clone()).or_insert(entry);
            }
            *old_path = path;
        }
        true
    });
    // A failed earlier upgrade may also have persisted an absolute sweep cursor.
    state.orphan_sweeps.retain(|cursor| {
        let safe = cursor.relative_parent.as_os_str().is_empty()
            || cursor
                .relative_parent
                .to_str()
                .is_some_and(|p| validate_relative(p).is_ok());
        changed |= !safe;
        safe
    });
    changed
}

/// Returns whether the state was absent, malformed, or from the shell-era
/// format. Only those rebuild paths justify scanning a bounded set of project
/// parents for private replacement artifacts.
fn read_state_anchored_with_sweep(state_directory: &StateDirectory) -> Result<(FilesState, bool)> {
    match state_directory.open_state() {
        Ok(Some(file)) => match serde_json::from_reader::<_, FilesState>(BufReader::new(file)) {
            Ok(state) => {
                let sweep = state.version == 0;
                Ok((state, sweep))
            }
            Err(error) => {
                // State is a cache. Ignoring malformed state is safe because
                // unknown old paths are retained rather than guessed at.
                tracing::warn!(path = %state_directory.path().display(), error = %error, "ignoring malformed files state");
                Ok((FilesState::default(), true))
            }
        },
        Ok(None) => Ok((FilesState::default(), true)),
        Err(error) => Err(error).into_diagnostic().wrap_err_with(|| {
            format!(
                "failed to read files state {}",
                state_directory.path().display()
            )
        }),
    }
}

fn write_state(state_directory: &StateDirectory, state: &FilesState) -> Result<()> {
    let (mut temporary, name) = state_directory.temporary().into_diagnostic()?;
    let write_result = serde_json::to_writer(&mut temporary, state)
        .into_diagnostic()
        .and_then(|()| temporary.write_all(b"\n").into_diagnostic());
    if let Err(error) = write_result {
        state_directory.remove_temporary(&name);
        return Err(error);
    }
    // This state is a recoverable reconciliation cache, not a transaction log.
    // Atomic replacement prevents torn JSON; a future journal can add power-loss durability.
    if let Err(error) = state_directory.replace(&name) {
        state_directory.remove_temporary(&name);
        return Err(error).into_diagnostic().wrap_err_with(|| {
            format!(
                "failed to replace files state {}",
                state_directory.path().display()
            )
        });
    }
    crash_test_failpoint("state");
    Ok(())
}

enum Cleanup {
    Removed,
    Absent,
    Retained,
}

fn cleanup_removed(
    resolver: &mut Resolver,
    relative: &str,
    old: Option<&ManagedFile>,
) -> Result<Cleanup> {
    let destination = match resolver.destination(relative, false) {
        Ok(destination) => destination,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Cleanup::Absent);
        }
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ELOOP | libc::ENOTDIR | libc::EXDEV)
            ) =>
        {
            // The old parent may now be a symlink. Forget ownership without
            // following it, rather than making every future shell entry fail.
            return Ok(Cleanup::Retained);
        }
        Err(error) => return Err(error).into_diagnostic(),
    };
    let Some(metadata) = destination.metadata().into_diagnostic()? else {
        return Ok(Cleanup::Absent);
    };
    if metadata.kind != rustix::fs::FileType::Symlink {
        return Ok(Cleanup::Retained);
    }
    let target = destination.read_link().into_diagnostic()?;
    let recorded_target_matches = old
        .and_then(|entry| entry.source.as_ref())
        .is_some_and(|source| source == &target);
    let legacy_store_link =
        old.and_then(|entry| entry.source.as_ref()).is_none() && target.starts_with("/nix/store");
    if !recorded_target_matches && !legacy_store_link {
        return Ok(Cleanup::Retained);
    }
    destination.remove_file().into_diagnostic()?;
    resolver.remove_empty_parents(relative);
    Ok(Cleanup::Removed)
}

#[derive(Clone, Copy)]
enum Decision {
    Unchanged,
    Created,
    Updated,
    Retained,
    Conflict,
}

struct ReconcileResult {
    decision: Decision,
    fingerprint: Option<Fingerprint>,
    source_fingerprint: Option<Fingerprint>,
    immutable_source: Option<PathBuf>,
}

#[derive(Clone)]
struct SourceFingerprint {
    fingerprint: Option<Fingerprint>,
    immutable_source: Option<PathBuf>,
}

impl From<Decision> for ReconcileResult {
    fn from(decision: Decision) -> Self {
        Self {
            decision,
            fingerprint: None,
            source_fingerprint: None,
            immutable_source: None,
        }
    }
}

fn reconcile_one(
    resolver: &mut Resolver,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    desired: &DesiredFile,
    old: Option<&ManagedFile>,
    source_fingerprints: &mut HashMap<PathBuf, SourceFingerprint>,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
) -> Result<ReconcileResult> {
    let destination = resolver
        .destination(&desired.path, true)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "parent directory is a symlink, not a directory, or changed while resolving {}; ensure every parent component is a real directory by replacing symlinked parents with real directories, or choose a destination without symlinked parents",
                desired.path,
            )
        })?;
    match desired.mode {
        CopyMode::Symlink => reconcile_symlink(
            &destination,
            state_directory,
            recovery,
            &desired.source,
            old,
            garbage,
            live_artifacts,
            cancellation,
        )
        .map(Into::into),
        CopyMode::Seed => reconcile_seed(
            &destination,
            state_directory,
            recovery,
            &desired.source,
            old,
            garbage,
            live_artifacts,
            cancellation,
        )
        .map(Into::into),
        CopyMode::Copy => reconcile_copy(
            &destination,
            state_directory,
            recovery,
            desired,
            old,
            source_fingerprints,
            garbage,
            live_artifacts,
            cancellation,
        ),
    }
}

fn begin_artifact(
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    phase: ArtifactPhase,
) -> Result<(String, Artifact)> {
    if recovery.artifacts.len() >= MAX_DEFERRED_CLEANUPS {
        return Err(miette!(
            "files recovery artifact backlog reached {MAX_DEFERRED_CLEANUPS} entries"
        ));
    }
    let id = Uuid::new_v4().to_string();
    let name = format!(".devenv-files-artifact-{id}");
    let intent = destination
        .artifact_intent(name.clone())
        .into_diagnostic()?;
    recovery.version = 1;
    recovery.push_artifact(RecoveryArtifact {
        id: id.clone(),
        intent,
        artifact: None,
        phase,
    });
    write_recovery(state_directory, recovery)?;
    let artifact = match destination.create_artifact(std::ffi::OsStr::new(&name)) {
        Ok(artifact) => artifact,
        Err(error) => {
            recovery.remove_artifact(&id);
            if let Err(journal_error) = write_recovery(state_directory, recovery) {
                return Err(journal_error).wrap_err_with(|| {
                    format!("failed to clear recovery intent after artifact error: {error}")
                });
            }
            return Err(error).into_diagnostic();
        }
    };
    crash_test_failpoint("mkdir");
    let record = recovery
        .artifacts
        .get_mut(&id)
        .expect("artifact intent was just inserted");
    record.artifact = Some(artifact.deferred());
    recovery.mark_artifact(&id);
    write_recovery(state_directory, recovery)?;
    crash_test_failpoint("journal");
    Ok((id, artifact))
}

fn forget_artifact(
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    id: &str,
) -> Result<()> {
    if recovery.remove_artifact(id) {
        write_recovery(state_directory, recovery)?;
    }
    Ok(())
}

fn reconcile_symlink(
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    source: &Path,
    old: Option<&ManagedFile>,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
) -> Result<Decision> {
    match destination.metadata().into_diagnostic()? {
        Some(metadata) if metadata.kind == rustix::fs::FileType::Symlink => {
            if destination.read_link().into_diagnostic()? == source {
                return Ok(Decision::Unchanged);
            }
            replace_symlink(
                source,
                destination,
                state_directory,
                recovery,
                garbage,
                live_artifacts,
                cancellation,
            )?;
            Ok(Decision::Updated)
        }
        Some(_) => {
            let expected_fingerprint = old
                .filter(|entry| entry.mode == Some(CopyMode::Copy))
                .and_then(|entry| entry.fingerprint.as_ref());
            let matches_managed_copy = if let Some(expected) = expected_fingerprint {
                destination.fingerprint(cancellation).into_diagnostic()? == *expected
            } else {
                false
            };
            if !matches_managed_copy {
                return Ok(Decision::Conflict);
            }
            replace_symlink(
                source,
                destination,
                state_directory,
                recovery,
                garbage,
                live_artifacts,
                cancellation,
            )?;
            Ok(Decision::Updated)
        }
        None => {
            destination.create_symlink(source).into_diagnostic()?;
            Ok(Decision::Created)
        }
    }
}

fn replace_symlink(
    source: &Path,
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
) -> Result<()> {
    let (artifact_id, artifact) = begin_artifact(
        destination,
        state_directory,
        recovery,
        ArtifactPhase::Staging,
    )?;
    let staged = destination
        .stage_symlink(artifact, source)
        .into_diagnostic()?;
    install_staged_replacement(
        destination,
        state_directory,
        recovery,
        garbage,
        live_artifacts,
        cancellation,
        artifact_id,
        staged,
    )
}

fn reconcile_seed(
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    source: &Path,
    old: Option<&ManagedFile>,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
) -> Result<Decision> {
    match destination.metadata().into_diagnostic()? {
        Some(metadata) if metadata.kind == rustix::fs::FileType::Symlink => {
            let target = destination.read_link().into_diagnostic()?;
            let was_managed = old
                .and_then(|entry| entry.source.as_ref())
                .is_some_and(|old_source| old_source == &target)
                || target.starts_with("/nix/store");
            if !was_managed {
                return Ok(Decision::Retained);
            }
            materialize_replace(
                source,
                destination,
                state_directory,
                recovery,
                garbage,
                live_artifacts,
                cancellation,
            )?;
            Ok(Decision::Updated)
        }
        Some(_) => Ok(Decision::Retained),
        None => {
            materialize_atomic(source, destination, state_directory, recovery, cancellation)?;
            Ok(Decision::Created)
        }
    }
}

fn reconcile_copy(
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    desired: &DesiredFile,
    old: Option<&ManagedFile>,
    source_fingerprints: &mut HashMap<PathBuf, SourceFingerprint>,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
) -> Result<ReconcileResult> {
    let source = &desired.source;
    let cached_immutable = old
        .filter(|old| {
            old.source.as_ref() == Some(source) && old.mode.as_ref() == Some(&CopyMode::Copy)
        })
        .and_then(|old| old.immutable_source.as_ref())
        .filter(|canonical| {
            fs::canonicalize(source).is_ok_and(|resolved| resolved == canonical.as_path())
        })
        .cloned();
    let source_state = if let Some(immutable_source) = cached_immutable {
        SourceFingerprint {
            fingerprint: None,
            immutable_source: Some(immutable_source),
        }
    } else if let Some(state) = source_fingerprints.get(source) {
        state.clone()
    } else {
        let immutable_source =
            fingerprint::immutable_store_source(source, cancellation).into_diagnostic()?;
        let fingerprint = if immutable_source.is_some() {
            None
        } else {
            Some(fingerprint::source_fingerprint(source, cancellation).into_diagnostic()?)
        };
        let state = SourceFingerprint {
            fingerprint,
            immutable_source,
        };
        source_fingerprints.insert(source.clone(), state.clone());
        state
    };
    let source_fingerprint = source_state.fingerprint;
    let immutable_source = source_state.immutable_source;
    let source_is_immutable = immutable_source.is_some();
    match destination.metadata().into_diagnostic()? {
        Some(metadata) if metadata.kind != rustix::fs::FileType::Symlink => {
            // Native watchers can miss mapped writes and writes through outside
            // hard links, so the full fingerprint remains the correctness check.
            let current = destination.fingerprint(cancellation).into_diagnostic()?;
            let fingerprint_matches = old.is_some_and(|old| {
                old.source.as_ref() == Some(source)
                    && old.mode.as_ref() == Some(&CopyMode::Copy)
                    && old.fingerprint.as_ref() == Some(&current)
                    && (if source_is_immutable {
                        old.immutable_source.as_ref() == immutable_source.as_ref()
                    } else {
                        old.source_fingerprint.as_ref() == source_fingerprint.as_ref()
                    })
            });
            // A recorded destination fingerprint makes any mismatch decisive.
            // Byte comparison is only needed while upgrading legacy state.
            let comparable = matches!(
                metadata.kind,
                rustix::fs::FileType::RegularFile | rustix::fs::FileType::Directory
            );
            let legacy_contents_match =
                if comparable && old.is_none_or(|old| old.fingerprint.is_none()) {
                    trees_equal(source, &destination.open().into_diagnostic()?, cancellation)?
                } else {
                    false
                };
            if fingerprint_matches || legacy_contents_match {
                return Ok(ReconcileResult {
                    decision: Decision::Unchanged,
                    fingerprint: Some(current),
                    source_fingerprint,
                    immutable_source,
                });
            }
            let fingerprint = materialize_replace(
                source,
                destination,
                state_directory,
                recovery,
                garbage,
                live_artifacts,
                cancellation,
            )?;
            Ok(ReconcileResult {
                decision: Decision::Updated,
                fingerprint,
                source_fingerprint,
                immutable_source,
            })
        }
        Some(_) => {
            let fingerprint = materialize_replace(
                source,
                destination,
                state_directory,
                recovery,
                garbage,
                live_artifacts,
                cancellation,
            )?;
            Ok(ReconcileResult {
                decision: Decision::Updated,
                fingerprint,
                source_fingerprint,
                immutable_source,
            })
        }
        None => {
            let fingerprint =
                materialize_atomic(source, destination, state_directory, recovery, cancellation)?;
            Ok(ReconcileResult {
                decision: Decision::Created,
                fingerprint,
                source_fingerprint,
                immutable_source,
            })
        }
    }
}

fn fingerprint_after_commit(
    destination: &Destination,
    staged_fingerprint: Fingerprint,
) -> Option<Fingerprint> {
    let result = destination.open().and_then(|destination| {
        fingerprint::refresh_root_fd(&destination, staged_fingerprint, &CancellationToken::new())
    });
    match result {
        Ok(fingerprint) => Some(fingerprint),
        Err(error) => {
            tracing::warn!(
                path = %destination.path().display(),
                error = %error,
                "could not cache the committed file fingerprint"
            );
            None
        }
    }
}

fn validate_relative(relative: &str) -> Result<()> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(miette!(
            "managed file path must be a clean relative path: {relative:?}"
        ));
    }
    Ok(())
}

fn materialize_replace(
    source: &Path,
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
) -> Result<Option<Fingerprint>> {
    if garbage.len() >= MAX_DEFERRED_CLEANUPS {
        let source_is_directory = fs::metadata(source).into_diagnostic()?.is_dir();
        let destination_is_directory = destination
            .metadata()
            .into_diagnostic()?
            .is_some_and(|metadata| metadata.kind == rustix::fs::FileType::Directory);
        if source_is_directory || destination_is_directory {
            return Err(miette!(
                "deferred file cleanup backlog reached {MAX_DEFERRED_CLEANUPS} entries"
            ));
        }
    }
    let (stage_id, staged, staged_fingerprint) = stage_materialization(
        source,
        destination,
        state_directory,
        recovery,
        ArtifactPhase::Staging,
        cancellation,
    )?;
    install_staged_replacement(
        destination,
        state_directory,
        recovery,
        garbage,
        live_artifacts,
        cancellation,
        stage_id,
        staged,
    )?;
    Ok(fingerprint_after_commit(destination, staged_fingerprint))
}

#[allow(clippy::too_many_arguments)]
fn install_staged_replacement(
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    garbage: &mut Vec<Deferred>,
    live_artifacts: &mut HashMap<Deferred, Artifact>,
    cancellation: &CancellationToken,
    stage_id: String,
    staged: Staged,
) -> Result<()> {
    if cancellation.is_cancelled() {
        let _ = staged.remove(&CancellationToken::new());
        let _ = forget_artifact(state_directory, recovery, &stage_id);
        return Err(miette!("files reconciliation was cancelled"));
    }

    let source_is_directory = staged.is_directory();
    let destination_metadata = destination
        .metadata()
        .into_diagnostic()?
        .ok_or_else(|| miette!("destination disappeared during replacement"))?;
    let destination_is_directory = destination_metadata.kind == rustix::fs::FileType::Directory;
    if (source_is_directory || destination_is_directory) && garbage.len() >= MAX_DEFERRED_CLEANUPS {
        let _ = staged.remove(&CancellationToken::new());
        let _ = forget_artifact(state_directory, recovery, &stage_id);
        return Err(miette!(
            "deferred file cleanup backlog reached {MAX_DEFERRED_CLEANUPS} entries"
        ));
    }
    if !source_is_directory && !destination_is_directory {
        if let Err(error) = destination.rename_from(&staged) {
            let _ = staged.remove(&CancellationToken::new());
            let _ = forget_artifact(state_directory, recovery, &stage_id);
            return Err(error).into_diagnostic();
        }
        staged.remove(&CancellationToken::new()).into_diagnostic()?;
        forget_artifact(state_directory, recovery, &stage_id)?;
        return Ok(());
    }

    let exchange_deferred = staged.deferred();
    match destination.exchange_from(&staged) {
        Ok(true) => {
            if let Some(record) = recovery.artifacts.get_mut(&stage_id) {
                record.phase = ArtifactPhase::Exchanged;
            }
            recovery.mark_artifact(&stage_id);
            write_recovery(state_directory, recovery)?;
            crash_test_failpoint("rename");
            garbage.push(exchange_deferred.clone());
            if garbage.len() <= MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN {
                live_artifacts.insert(exchange_deferred, staged.into_artifact());
            }
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            let _ = staged.remove(&CancellationToken::new());
            let _ = forget_artifact(state_directory, recovery, &stage_id);
            return Err(error).into_diagnostic();
        }
    }
    // Older filesystems may not support swap renames. Keep the fallback
    // descriptor-relative; the project lock limits its brief missing-name
    // window to non-cooperating filesystem writers.
    let (backup_id, backup_artifact) = begin_artifact(
        destination,
        state_directory,
        recovery,
        ArtifactPhase::Backup,
    )?;
    let mut backup = match destination.stage_placeholder(backup_artifact, destination_metadata.kind)
    {
        Ok(backup) => backup,
        Err(error) => {
            let _ = staged.remove(&CancellationToken::new());
            let _ = forget_artifact(state_directory, recovery, &stage_id);
            let _ = forget_artifact(state_directory, recovery, &backup_id);
            return Err(error).into_diagnostic();
        }
    };
    let backup_deferred = backup.deferred();
    let staged_identity = staged.identity();
    let placeholder = backup.identity();
    if recovery.fallbacks.len() >= MAX_DEFERRED_CLEANUPS {
        let _ = staged.remove(&CancellationToken::new());
        let _ = backup.remove(&CancellationToken::new());
        let _ = forget_artifact(state_directory, recovery, &stage_id);
        let _ = forget_artifact(state_directory, recovery, &backup_id);
        return Err(miette!(
            "files recovery fallback backlog reached {MAX_DEFERRED_CLEANUPS} entries"
        ));
    }
    recovery.push_fallback(FallbackRecovery {
        destination: destination.relative_path().to_string_lossy().into_owned(),
        stage_id: stage_id.clone(),
        backup_id: backup_id.clone(),
        original: entry_identity(destination_metadata),
        staged: entry_identity(staged_identity),
        placeholder: entry_identity(placeholder),
        // This is deliberately recorded before moving the old name. Recovery
        // treats it as "may have happened" and verifies both identities.
        phase: FallbackPhase::MayMoveOld,
    });
    write_recovery(state_directory, recovery)?;
    if let Err(error) = destination.rename_to(&mut backup) {
        let staged_removed = staged.remove(&CancellationToken::new()).is_ok();
        let backup_removed = backup.remove(&CancellationToken::new()).is_ok();
        recovery.remove_fallback(&stage_id);
        if staged_removed {
            recovery.remove_artifact(&stage_id);
        }
        if backup_removed {
            recovery.remove_artifact(&backup_id);
        }
        if let Err(journal_error) = write_recovery(state_directory, recovery) {
            return Err(journal_error).wrap_err_with(|| {
                format!("failed to record cleanup after replacement error: {error}")
            });
        }
        return Err(error).into_diagnostic();
    }
    if let Some(fallback) = recovery.fallbacks.get_mut(&stage_id) {
        fallback.phase = FallbackPhase::MayInstallNew;
    }
    recovery.mark_fallback(&stage_id);
    write_recovery(state_directory, recovery)?;
    if let Err(error) = destination.rename_from(&staged) {
        let rollback = destination.rename_from(&backup);
        if let Err(rollback_error) = rollback {
            return Err(miette!(
                "failed to install replacement ({error}) and restore previous destination ({rollback_error})"
            ));
        }
        let staged_removed = staged.remove(&CancellationToken::new()).is_ok();
        let backup_removed = backup.remove(&CancellationToken::new()).is_ok();
        recovery.remove_fallback(&stage_id);
        if staged_removed {
            recovery.remove_artifact(&stage_id);
        }
        if backup_removed {
            recovery.remove_artifact(&backup_id);
        }
        write_recovery(state_directory, recovery).wrap_err_with(|| {
            format!("failed to record rollback after replacement error: {error}")
        })?;
        return Err(error).into_diagnostic();
    }
    crash_test_failpoint("rename");
    garbage.push(backup_deferred.clone());
    if garbage.len() <= MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN {
        live_artifacts.insert(backup_deferred, backup.into_artifact());
    }
    staged.remove(&CancellationToken::new()).into_diagnostic()?;
    recovery.remove_fallback(&stage_id);
    recovery.remove_artifact(&stage_id);
    write_recovery(state_directory, recovery)?;
    Ok(())
}

fn materialize_atomic(
    source: &Path,
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    cancellation: &CancellationToken,
) -> Result<Option<Fingerprint>> {
    let (stage_id, staged, staged_fingerprint) = stage_materialization(
        source,
        destination,
        state_directory,
        recovery,
        ArtifactPhase::Staging,
        cancellation,
    )?;
    if cancellation.is_cancelled() {
        let _ = staged.remove(&CancellationToken::new());
        let _ = forget_artifact(state_directory, recovery, &stage_id);
        return Err(miette!("files reconciliation was cancelled"));
    }
    if let Err(error) = destination.rename_noreplace_from(&staged) {
        let _ = staged.remove(&CancellationToken::new());
        let _ = forget_artifact(state_directory, recovery, &stage_id);
        return Err(error).into_diagnostic();
    }
    staged.remove(&CancellationToken::new()).into_diagnostic()?;
    forget_artifact(state_directory, recovery, &stage_id)?;
    Ok(fingerprint_after_commit(destination, staged_fingerprint))
}

fn stage_materialization(
    source: &Path,
    destination: &Destination,
    state_directory: &StateDirectory,
    recovery: &mut RecoveryJournal,
    phase: ArtifactPhase,
    cancellation: &CancellationToken,
) -> Result<(String, Staged, Fingerprint)> {
    let metadata = fs::metadata(source)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to inspect source {}", source.display()))?;
    if metadata.is_dir() {
        let canonical_source = fs::canonicalize(source).into_diagnostic()?;
        let destination_parent = destination
            .path()
            .parent()
            .ok_or_else(|| miette!("destination has no parent"))?;
        let canonical_parent = fs::canonicalize(destination_parent).into_diagnostic()?;
        if canonical_parent.starts_with(&canonical_source) {
            return Err(miette!(
                "cannot copy a directory into itself: {}",
                source.display()
            ));
        }
    }
    let mut visited = HashSet::new();
    let (artifact_id, artifact) = begin_artifact(destination, state_directory, recovery, phase)?;

    if metadata.is_dir() {
        let (staged, directory) = destination.stage_directory(artifact).into_diagnostic()?;
        let mut fingerprint = fingerprint::CopiedTreeFingerprintBuilder::new();
        let result = copy_directory_contents(
            source,
            Path::new(""),
            &metadata,
            &directory,
            &mut visited,
            &mut fingerprint,
            cancellation,
        )
        .and_then(|()| set_writable_permissions_fd(&directory, &metadata))
        .and_then(|()| {
            fingerprint
                .finish(&directory, cancellation)
                .into_diagnostic()
        });
        match result {
            Ok(fingerprint) => Ok((artifact_id, staged, fingerprint)),
            Err(error) => {
                let _ = staged.remove(&CancellationToken::new());
                let _ = forget_artifact(state_directory, recovery, &artifact_id);
                Err(error)
            }
        }
    } else if metadata.is_file() {
        let (staged, file) = destination
            .stage_file(artifact, source, cancellation)
            .into_diagnostic()?;
        let result = set_writable_permissions_fd(&file, &metadata)
            .and_then(|()| fingerprint::fingerprint_fd(&file, cancellation).into_diagnostic());
        match result {
            Ok(fingerprint) => Ok((artifact_id, staged, fingerprint)),
            Err(error) => {
                let _ = staged.remove(&CancellationToken::new());
                let _ = forget_artifact(state_directory, recovery, &artifact_id);
                Err(error)
            }
        }
    } else {
        Err(miette!("source is neither a regular file nor directory"))
    }
}

fn copy_directory_contents(
    source: &Path,
    relative: &Path,
    source_metadata: &fs::Metadata,
    destination: &OwnedFd,
    visited: &mut HashSet<(u64, u64)>,
    fingerprint: &mut fingerprint::CopiedTreeFingerprintBuilder,
    cancellation: &CancellationToken,
) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(miette!("files reconciliation was cancelled"));
    }
    let identity = (source_metadata.dev(), source_metadata.ino());
    if !visited.insert(identity) {
        return Err(miette!("source directory contains a symlink cycle"));
    }
    let mut entries = fs::read_dir(source)
        .into_diagnostic()?
        .collect::<std::io::Result<Vec<_>>>()
        .into_diagnostic()?;
    entries.sort_unstable_by_key(|entry| entry.file_name());
    for entry in entries {
        if cancellation.is_cancelled() {
            return Err(miette!("files reconciliation was cancelled"));
        }
        let source_path = entry.path();
        let name = entry.file_name();
        let child_relative = relative.join(&name);
        let fingerprint_index = fingerprint.begin_entry(&child_relative);
        let metadata = fs::metadata(&source_path).into_diagnostic()?;
        if metadata.is_dir() {
            unix_fs::mkdirat(destination, &name, Mode::from_bits_retain(0o700))
                .map_err(std::io::Error::from)
                .into_diagnostic()?;
            let child = unix_fs::openat(
                destination,
                &name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)
            .into_diagnostic()?;
            copy_directory_contents(
                &source_path,
                &child_relative,
                &metadata,
                &child,
                visited,
                fingerprint,
                cancellation,
            )?;
            set_writable_permissions_fd(&child, &metadata)?;
            fingerprint
                .finish_entry(fingerprint_index, &child, cancellation)
                .into_diagnostic()?;
        } else if metadata.is_file() {
            let file =
                platform::copy_regular_file_at(&source_path, destination, &name, cancellation)
                    .into_diagnostic()?;
            set_writable_permissions_fd(&file, &metadata)?;
            fingerprint
                .finish_entry(fingerprint_index, &file, cancellation)
                .into_diagnostic()?;
        } else {
            return Err(miette!("source tree contains an unsupported file type"));
        }
    }
    visited.remove(&identity);
    Ok(())
}

fn set_writable_permissions_fd(fd: &impl AsFd, source: &fs::Metadata) -> Result<()> {
    unix_fs::fchmod(
        fd,
        Mode::from_raw_mode((source.permissions().mode() | 0o200) as _),
    )
    .map_err(std::io::Error::from)
    .into_diagnostic()
}

fn trees_equal(
    source: &Path,
    destination: &OwnedFd,
    cancellation: &CancellationToken,
) -> Result<bool> {
    if cancellation.is_cancelled() {
        return Err(miette!("files reconciliation was cancelled"));
    }
    let source_meta = fs::metadata(source).into_diagnostic()?;
    let destination_meta = File::from(rustix::io::dup(destination).into_diagnostic()?)
        .metadata()
        .into_diagnostic()?;
    if source_meta.is_file() != destination_meta.is_file()
        || source_meta.is_dir() != destination_meta.is_dir()
        || !permissions_equal(&source_meta, &destination_meta)
    {
        return Ok(false);
    }
    if source_meta.is_file() {
        return files_equal(
            source,
            File::from(rustix::io::dup(destination).into_diagnostic()?),
            cancellation,
        );
    }

    let mut source_entries = 0_usize;
    for entry in fs::read_dir(source).into_diagnostic()? {
        if cancellation.is_cancelled() {
            return Err(miette!("files reconciliation was cancelled"));
        }
        let entry = entry.into_diagnostic()?;
        source_entries += 1;
        let name = entry.file_name();
        let stat = match unix_fs::statat(destination, &name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
        {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => return Ok(false),
            Err(error) => return Err(std::io::Error::from(error)).into_diagnostic(),
        };
        let kind = rustix::fs::FileType::from_raw_mode(stat.st_mode);
        if !matches!(
            kind,
            rustix::fs::FileType::RegularFile | rustix::fs::FileType::Directory
        ) {
            return Ok(false);
        }
        let child = unix_fs::openat(
            destination,
            &name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)
        .into_diagnostic()?;
        if !trees_equal(&entry.path(), &child, cancellation)? {
            return Ok(false);
        }
    }
    let mut destination_entries = 0_usize;
    let mut directory = unix_fs::Dir::read_from(destination)
        .map_err(std::io::Error::from)
        .into_diagnostic()?;
    while let Some(entry) = directory.read() {
        let entry = entry.map_err(std::io::Error::from).into_diagnostic()?;
        if !matches!(entry.file_name().to_bytes(), b"." | b"..") {
            destination_entries += 1;
        }
    }
    Ok(source_entries == destination_entries)
}

fn permissions_equal(source: &fs::Metadata, destination: &fs::Metadata) -> bool {
    destination.permissions().mode() & 0o7777 == (source.permissions().mode() | 0o200) & 0o7777
}

fn files_equal(left: &Path, mut right: File, cancellation: &CancellationToken) -> Result<bool> {
    let mut left = File::open(left).into_diagnostic()?;
    if left.metadata().into_diagnostic()?.len() != right.metadata().into_diagnostic()?.len() {
        return Ok(false);
    }
    let mut left_buffer = vec![0_u8; 256 * 1024];
    let mut right_buffer = vec![0_u8; 256 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(miette!("files reconciliation was cancelled"));
        }
        let left_read = left.read(&mut left_buffer).into_diagnostic()?;
        let right_read = right.read(&mut right_buffer).into_diagnostic()?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests;
