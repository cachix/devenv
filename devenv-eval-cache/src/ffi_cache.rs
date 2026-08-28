//! FFI-based eval caching types for NixCBackend.
//!
//! This module provides core types for caching evaluation results
//! when using the FFI backend instead of the CLI backend.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::eval_inputs::{EnvInputDesc, FileInputDesc, Input};
use devenv_core::eval_op::{EvalInputState, EvalOp, OpObserver};
pub use devenv_core::nix_args::NixArgs;
use sha2::{Digest, Sha256};

/// Cache key for an evaluation operation.
///
/// The key is computed from NixArgs (all eval configuration) plus the attribute name,
/// providing a unique identifier for each distinct evaluation. The import expression
/// itself is not included in the key since it's tracked via observed file inputs.
#[derive(Clone, Debug)]
pub struct EvalCacheKey {
    /// Hash of serialized NixArgs + attr_name
    pub key_hash: String,
    /// Human-readable attribute name for debugging
    pub attr_name: String,
}

impl EvalCacheKey {
    /// Create a new cache key from NixArgs and attribute name.
    ///
    /// The key captures all evaluation configuration (system, paths, profiles, etc.)
    /// plus the specific attribute being evaluated. The import expression is not
    /// included here because it's tracked as an observed file input during evaluation.
    pub fn new(nix_args: &NixArgs, attr_name: &str) -> Self {
        let nix_args_str = ser_nix::to_string(nix_args).unwrap_or_default();
        Self::from_nix_args_str(&nix_args_str, attr_name)
    }

    /// Create a cache key from a pre-serialized NixArgs string.
    ///
    /// This is useful when the NixArgs has already been serialized (e.g., during
    /// assemble() and stored for later use in cache key generation).
    pub fn from_nix_args_str(nix_args_str: &str, attr_name: &str) -> Self {
        let combined = format!("{}:{}", nix_args_str, attr_name);
        let key_hash = devenv_cache_core::compute_string_hash(&combined);
        Self {
            key_hash,
            attr_name: attr_name.to_string(),
        }
    }

    /// Create a cache key from a raw string for testing.
    ///
    /// This allows creating keys without full NixArgs, useful for testing the caching
    /// service independently from the key computation logic.
    #[cfg(test)]
    pub fn from_test_string(raw_key: &str, attr_name: &str) -> Self {
        Self::from_nix_args_str(raw_key, attr_name)
    }
}

/// Configuration for eval caching behavior.
#[derive(Clone, Debug, Default)]
pub struct CachingConfig {
    /// Force re-evaluation even if cache is valid.
    pub force_refresh: bool,
    /// Additional paths to watch for changes beyond those detected during eval.
    pub extra_watch_paths: Vec<PathBuf>,
    /// Paths to exclude from cache invalidation (e.g., generated files).
    pub excluded_paths: Vec<PathBuf>,
    /// Environment variable names to exclude from cache invalidation
    /// (e.g., vars already tracked via NixArgs).
    pub excluded_envs: Vec<String>,
}

/// Long-lived accumulator of distinct file/env operations observed during Nix
/// evaluation.
///
/// Registered once on `NixLogBridge` for the lifetime of a `CachingEvalState`.
/// Callers `snapshot_inputs()` at cache-miss store time and `clear()` when the
/// underlying `EvalState` is invalidated (e.g. hot-reload).
///
/// Ops are deduplicated at insertion: Nix's internal `fileEvalCache` already
/// suppresses same-session re-parses, but env-var accesses and `pathExists`
/// checks can re-fire across attribute evaluations. The set keeps memory
/// bounded to the distinct file/env universe of the session rather than the
/// raw event count.
pub struct EvalInputTracker {
    ops: Mutex<HashSet<EvalOp>>,
    restored: Mutex<EvalInputIdentities>,
    precise: Mutex<HashSet<EvalInputState>>,
    config: Mutex<CachingConfig>,
}

#[derive(Clone, Debug, Default)]
struct EvalInputIdentities {
    paths: BTreeMap<PathBuf, bool>,
    envs: BTreeSet<String>,
}

impl EvalInputIdentities {
    fn clear(&mut self) {
        self.paths.clear();
        self.envs.clear();
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.envs.is_empty()
    }

    fn insert_path(&mut self, path: PathBuf, recursive: bool) {
        self.paths
            .entry(path)
            .and_modify(|existing| *existing |= recursive)
            .or_insert(recursive);
    }

    fn merge(&mut self, other: Self) {
        for (path, recursive) in other.paths {
            self.insert_path(path, recursive);
        }
        self.envs.extend(other.envs);
    }
}

impl EvalInputTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ops: Mutex::new(HashSet::new()),
            restored: Mutex::new(EvalInputIdentities::default()),
            precise: Mutex::new(HashSet::new()),
            config: Mutex::new(CachingConfig::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<EvalOp>> {
        self.ops.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Clear the tracked ops. The tracker stays registered as an observer.
    pub fn clear(&self) {
        self.lock().clear();
        self.restored
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        self.precise
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    /// Update filtering and extra-watch configuration for future snapshots.
    pub fn set_config(&self, config: CachingConfig) {
        *self
            .config
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = config;
    }

    /// Snapshot every observed identity into fresh file/env descriptors.
    pub fn snapshot_inputs(&self) -> Vec<Input> {
        let config = self
            .config
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let mut identities = ops_to_identities(self.lock().iter().cloned(), &config);
        let restored = self
            .restored
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        identities.merge(restored);
        identities_to_inputs(identities)
    }

    /// Merge identities restored from a cache hit into this EvalState session.
    /// Descriptors are deliberately recaptured by [`Self::snapshot_inputs`]
    /// rather than retained, so later misses never mix stale and fresh hashes.
    pub fn restore_inputs(&self, inputs: &[Input]) {
        let mut restored = self
            .restored
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for input in inputs {
            match input {
                Input::File(file) => restored.insert_path(file.path.clone(), file.recursive),
                Input::Env(env) => {
                    restored.envs.insert(env.name.clone());
                }
            }
        }
    }

    /// Return whether any input in a checkpoint changed.
    pub fn inputs_changed(&self, inputs: &[Input]) -> std::io::Result<bool> {
        for input in inputs {
            let changed = match input {
                Input::File(file) => {
                    let current =
                        FileInputDesc::new(file.path.clone(), SystemTime::now(), file.recursive)?;
                    current.content_hash != file.content_hash
                        || current.is_directory != file.is_directory
                }
                Input::Env(env) => {
                    EnvInputDesc::new(env.name.clone())?.content_hash != env.content_hash
                }
            };
            if changed {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Return whether any precisely observed primop input changed since it was
    /// consumed. Cache stores must refuse stale results when this is true.
    pub fn observed_inputs_changed(&self) -> std::io::Result<bool> {
        for observed in self
            .precise
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            let current = match observed {
                EvalInputState::File { path, .. } => EvalInputState::File {
                    path: path.clone(),
                    content_sha256: match std::fs::read(path) {
                        Ok(contents) => Some(hex::encode(Sha256::digest(contents))),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => return Err(error),
                    },
                },
                EvalInputState::Env { name, .. } => EvalInputState::Env {
                    name: name.clone(),
                    content_sha256: std::env::var(name)
                        .ok()
                        .map(|value| hex::encode(Sha256::digest(value.as_bytes()))),
                },
            };
            if current != *observed {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn clear_observed_input_states(&self) {
        self.precise
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    /// Snapshot the tracked ops as a `Vec` (for tests and diagnostics).
    pub fn snapshot(&self) -> Vec<EvalOp> {
        self.lock().iter().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
            && self
                .restored
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
            && self
                .precise
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
    }
}

impl OpObserver for EvalInputTracker {
    fn record(&self, op: EvalOp) {
        self.lock().insert(op);
    }

    fn record_input_state(&self, input: EvalInputState) {
        self.record(input.operation());
        self.precise
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(input);
    }
}

fn identities_to_inputs(identities: EvalInputIdentities) -> Vec<Input> {
    let fallback_time = SystemTime::now();
    let mut inputs = Vec::with_capacity(identities.paths.len() + identities.envs.len());
    inputs.extend(
        identities
            .paths
            .into_iter()
            .filter_map(|(path, recursive)| {
                FileInputDesc::new(path, fallback_time, recursive)
                    .ok()
                    .map(Input::File)
            }),
    );
    inputs.extend(
        identities
            .envs
            .into_iter()
            .filter_map(|name| EnvInputDesc::new(name).ok().map(Input::Env)),
    );
    inputs
}

/// Convert a list of operations to Input descriptors.
///
/// This is the core conversion logic that:
/// 1. Filters out irrelevant paths (nix store, excluded, non-absolute)
/// 2. Coalesces file operations by path, with copied-source tracking taking precedence
/// 3. Creates `FileInputDesc` and `EnvInputDesc` values
/// 4. Adds extra watch paths
pub fn ops_to_inputs(ops: impl IntoIterator<Item = EvalOp>, config: &CachingConfig) -> Vec<Input> {
    identities_to_inputs(ops_to_identities(ops, config))
}

fn ops_to_identities(
    ops: impl IntoIterator<Item = EvalOp>,
    config: &CachingConfig,
) -> EvalInputIdentities {
    let mut identities = EvalInputIdentities::default();

    for op in ops {
        let (source, recursive) = match op {
            EvalOp::ReadFile { source }
            | EvalOp::ReadDir { source }
            | EvalOp::ReadFileType { source }
            | EvalOp::HashFile { source, .. }
            | EvalOp::PathExists { source }
            | EvalOp::EvaluatedFile { source, .. } => (source, false),
            EvalOp::CopiedSource { source, .. } | EvalOp::FilteredSource { source, .. } => {
                (source, true)
            }
            EvalOp::GetEnv { name } => {
                if !config.excluded_envs.contains(&name) {
                    identities.envs.insert(name);
                }
                continue;
            }
        };

        if source.starts_with("/nix/store")
            || !source.is_absolute()
            || config
                .excluded_paths
                .iter()
                .any(|excluded| source.starts_with(excluded))
        {
            continue;
        }

        identities.insert_path(source, recursive);
    }

    // Add extra watch paths. These are meant to trigger re-evaluation on any
    // change, so watch directories recursively.
    for path in &config.extra_watch_paths {
        identities.insert_path(path.clone(), true);
    }
    identities
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Note: EvalCacheKey tests require NixArgs which is complex to construct in unit tests.
    // Key determinism and differentiation are tested through integration tests.

    #[test]
    fn test_tracker_starts_empty() {
        let tracker = EvalInputTracker::new();
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_push_and_snapshot() {
        let tracker = EvalInputTracker::new();
        tracker.record(EvalOp::GetEnv {
            name: "FOO".to_string(),
        });
        assert_eq!(tracker.snapshot().len(), 1);
        // Snapshot is non-destructive.
        assert_eq!(tracker.snapshot().len(), 1);
    }

    #[test]
    fn test_tracker_deduplicates_on_insert() {
        let tracker = EvalInputTracker::new();
        tracker.record(EvalOp::GetEnv {
            name: "A".to_string(),
        });
        tracker.record(EvalOp::GetEnv {
            name: "A".to_string(),
        });
        tracker.record(EvalOp::GetEnv {
            name: "B".to_string(),
        });
        assert_eq!(tracker.snapshot().len(), 2);
    }

    #[test]
    fn test_tracker_clear() {
        let tracker = EvalInputTracker::new();
        tracker.record(EvalOp::GetEnv {
            name: "FOO".to_string(),
        });
        tracker.clear();
        assert!(tracker.is_empty());
        // Still usable after clear.
        tracker.record(EvalOp::GetEnv {
            name: "BAR".to_string(),
        });
        assert_eq!(tracker.snapshot().len(), 1);
    }

    #[test]
    fn restored_paths_merge_recursive_observations_before_capture() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("file"), b"contents").unwrap();
        let path = temp_dir.path().to_path_buf();
        let fallback = SystemTime::now();
        let tracker = EvalInputTracker::new();
        tracker.restore_inputs(&[
            Input::File(FileInputDesc::new(path.clone(), fallback, false).unwrap()),
            Input::File(FileInputDesc::new(path.clone(), fallback, true).unwrap()),
        ]);

        let inputs = tracker.snapshot_inputs();
        assert_eq!(inputs.len(), 1);
        assert!(matches!(&inputs[0], Input::File(file) if file.path == path && file.recursive));
    }

    #[test]
    fn restored_inputs_are_recaptured_from_live_state() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("file");
        std::fs::write(&path, b"before").unwrap();
        let previous = FileInputDesc::new(path.clone(), SystemTime::now(), false).unwrap();
        let old_hash = previous.content_hash.clone();
        let tracker = EvalInputTracker::new();
        tracker.restore_inputs(&[Input::File(previous)]);

        std::fs::write(&path, b"after").unwrap();
        let inputs = tracker.snapshot_inputs();
        assert!(matches!(&inputs[0], Input::File(file) if file.content_hash != old_hash));
    }

    #[test]
    fn test_ops_to_inputs_filters_nix_store() {
        let ops = vec![EvalOp::ReadFile {
            source: PathBuf::from("/nix/store/abc123-foo/bar.txt"),
        }];
        let inputs = ops_to_inputs(ops, &CachingConfig::default());
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_ops_to_inputs_filters_non_absolute() {
        let ops = vec![EvalOp::ReadFile {
            source: PathBuf::from("relative/path.txt"),
        }];
        let inputs = ops_to_inputs(ops, &CachingConfig::default());
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_ops_to_inputs_filters_excluded() {
        let config = CachingConfig {
            excluded_paths: vec![PathBuf::from("/excluded")],
            ..Default::default()
        };
        let ops = vec![EvalOp::ReadFile {
            source: PathBuf::from("/excluded/file.txt"),
        }];
        let inputs = ops_to_inputs(ops, &config);
        assert!(inputs.is_empty());
    }

    #[test]
    fn test_ops_to_inputs_filters_excluded_envs() {
        let config = CachingConfig {
            excluded_envs: vec!["NIXPKGS_CONFIG".to_string()],
            ..Default::default()
        };
        let ops = vec![
            EvalOp::GetEnv {
                name: "NIXPKGS_CONFIG".to_string(),
            },
            EvalOp::GetEnv {
                name: "OTHER_VAR".to_string(),
            },
        ];
        let inputs = ops_to_inputs(ops, &config);
        // NIXPKGS_CONFIG should be filtered out, only OTHER_VAR remains
        assert_eq!(inputs.len(), 1);
        assert!(matches!(inputs[0], Input::Env(ref e) if e.name == "OTHER_VAR"));
    }

    #[test]
    fn test_ops_to_inputs_converts_env() {
        let ops = vec![EvalOp::GetEnv {
            name: "MY_VAR".to_string(),
        }];
        let inputs = ops_to_inputs(ops, &CachingConfig::default());
        assert_eq!(inputs.len(), 1);
        assert!(matches!(inputs[0], Input::Env(ref e) if e.name == "MY_VAR"));
    }

    #[test]
    fn test_ops_to_inputs_coalesces_path_with_copied_source_precedence() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("source.txt"), b"source").unwrap();
        let source = temp_dir.path().to_path_buf();
        let ops = vec![
            EvalOp::ReadDir {
                source: source.clone(),
            },
            EvalOp::CopiedSource {
                source: source.clone(),
                target: PathBuf::from("/nix/store/example-source"),
            },
        ];

        let inputs = ops_to_inputs(ops, &CachingConfig::default());

        assert_eq!(inputs.len(), 1);
        assert!(
            matches!(&inputs[0], Input::File(desc) if desc.path == source && desc.recursive),
            "a copied-source observation must subsume a weaker readDir observation"
        );
    }

    #[test]
    fn test_ops_to_inputs_tracks_new_file_effects_and_filtered_sources() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("source.txt");
        std::fs::write(&file, b"source").unwrap();
        let source = temp_dir.path().to_path_buf();

        let inputs = ops_to_inputs(
            vec![
                EvalOp::ReadFileType {
                    source: file.clone(),
                },
                EvalOp::HashFile {
                    source: file.clone(),
                    algorithm: "sha256".into(),
                },
                EvalOp::FilteredSource {
                    source: source.clone(),
                    target: PathBuf::from("/nix/store/example-source"),
                },
            ],
            &CachingConfig::default(),
        );

        assert_eq!(inputs.len(), 2);
        assert!(matches!(
            &inputs[0],
            Input::File(desc) if desc.path == source && desc.recursive
        ));
        assert!(matches!(
            &inputs[1],
            Input::File(desc) if desc.path == file && !desc.recursive
        ));
    }
}
