//! FFI-based eval caching types for NixCBackend.
//!
//! This module provides core types for caching evaluation results
//! when using the FFI backend instead of the CLI backend.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::eval_inputs::{EnvInputDesc, FileInputDesc, Input};
use devenv_core::eval_op::{EvalOp, OpObserver};
pub use devenv_core::nix_args::NixArgs;

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
    /// Prefix-matched: any source whose path starts with one of these is
    /// dropped, unless `excluded_path_exceptions` carves it back in by a
    /// longer (more specific) prefix.
    pub excluded_paths: Vec<PathBuf>,
    /// Carve-outs that override `excluded_paths`. Combined with
    /// `excluded_paths` under longest-prefix-match: for each source, the
    /// most specific matching entry wins. Ties favor the exception so a
    /// broad exclude with an equal-depth exception is still tracked. This
    /// lets callers exclude a parent broadly, carve out a subdirectory,
    /// and then re-exclude a leaf inside it by adding a longer entry to
    /// `excluded_paths` (e.g. exclude `.devenv/`, keep `.devenv/state/`,
    /// re-exclude `.devenv/state/tasks.db`).
    pub excluded_path_exceptions: Vec<PathBuf>,
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
pub struct InputTracker {
    ops: Mutex<HashSet<EvalOp>>,
}

impl InputTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ops: Mutex::new(HashSet::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<EvalOp>> {
        self.ops.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Clear the tracked ops. The tracker stays registered as an observer.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Snapshot current ops and convert them to `Input` descriptors.
    pub fn snapshot_inputs(&self, config: &CachingConfig) -> Vec<Input> {
        ops_to_inputs(self.lock().iter().cloned(), config)
    }

    /// Snapshot the tracked ops as a `Vec` (for tests and diagnostics).
    pub fn snapshot(&self) -> Vec<EvalOp> {
        self.lock().iter().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

impl OpObserver for InputTracker {
    fn record(&self, op: EvalOp) {
        self.lock().insert(op);
    }
}

/// Convert a list of operations to Input descriptors.
///
/// This is the core conversion logic that:
/// 1. Filters out irrelevant paths (nix store, excluded, non-absolute)
/// 2. Creates `FileInputDesc` for file operations
/// 3. Creates `EnvInputDesc` for environment variable access
/// 4. Adds extra watch paths
/// 5. Deduplicates the result
pub fn ops_to_inputs(ops: impl IntoIterator<Item = EvalOp>, config: &CachingConfig) -> Vec<Input> {
    let fallback_time = SystemTime::now();
    let mut inputs: Vec<Input> = Vec::new();

    for op in ops {
        match op {
            EvalOp::ReadFile { source }
            | EvalOp::ReadDir { source }
            | EvalOp::PathExists { source }
            | EvalOp::EvaluatedFile { source }
            | EvalOp::TrackedPath { source }
            | EvalOp::CopiedSource { source, .. } => {
                // Skip nix store paths (immutable)
                if source.starts_with("/nix/store") {
                    continue;
                }

                // Skip non-absolute paths
                if !source.is_absolute() {
                    continue;
                }

                // Longest-prefix-match between `excluded_paths` and
                // `excluded_path_exceptions`. The most specific matching
                // rule wins; ties favor the exception. This lets callers
                // re-exclude a leaf inside an otherwise-allowed carve-out
                // (e.g. exclude `.devenv/`, allow `.devenv/state/`,
                // re-exclude `.devenv/state/tasks.db`).
                let best_excluded = config
                    .excluded_paths
                    .iter()
                    .filter(|p| source.starts_with(p))
                    .map(|p| p.components().count())
                    .max();
                let best_allowed = config
                    .excluded_path_exceptions
                    .iter()
                    .filter(|p| source.starts_with(p))
                    .map(|p| p.components().count())
                    .max();
                let drop = match (best_excluded, best_allowed) {
                    (None, _) => false,
                    (Some(_), None) => true,
                    (Some(e), Some(a)) => e > a,
                };
                if drop {
                    continue;
                }

                // Create file input descriptor
                if let Ok(desc) = FileInputDesc::new(source, fallback_time) {
                    inputs.push(Input::File(desc));
                }
            }
            EvalOp::GetEnv { name } => {
                // Skip excluded env vars (already tracked elsewhere, e.g., via NixArgs)
                if config.excluded_envs.contains(&name) {
                    continue;
                }
                // Create env input descriptor
                if let Ok(desc) = EnvInputDesc::new(name) {
                    inputs.push(Input::Env(desc));
                }
            }
        }
    }

    // Add extra watch paths
    for path in &config.extra_watch_paths {
        if let Ok(desc) = FileInputDesc::new(path.clone(), fallback_time) {
            inputs.push(Input::File(desc));
        }
    }

    // Sort and deduplicate
    inputs.sort();
    inputs.dedup_by(Input::dedup);

    inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: EvalCacheKey tests require NixArgs which is complex to construct in unit tests.
    // Key determinism and differentiation are tested through integration tests.

    #[test]
    fn test_tracker_starts_empty() {
        let tracker = InputTracker::new();
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_push_and_snapshot() {
        let tracker = InputTracker::new();
        tracker.record(EvalOp::GetEnv {
            name: "FOO".to_string(),
        });
        assert_eq!(tracker.snapshot().len(), 1);
        // Snapshot is non-destructive.
        assert_eq!(tracker.snapshot().len(), 1);
    }

    #[test]
    fn test_tracker_deduplicates_on_insert() {
        let tracker = InputTracker::new();
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
        let tracker = InputTracker::new();
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
    fn test_ops_to_inputs_excluded_path_exceptions_kept() {
        // Broad exclude with a narrow carve-out: anything under /excluded is
        // dropped, except /excluded/keep — used to ignore devenv's own state
        // dir while still tracking user files under $DEVENV_STATE.
        let config = CachingConfig {
            excluded_paths: vec![PathBuf::from("/excluded")],
            excluded_path_exceptions: vec![PathBuf::from("/excluded/keep")],
            ..Default::default()
        };
        let ops = vec![
            EvalOp::ReadFile {
                source: PathBuf::from("/excluded/internal.db"),
            },
            EvalOp::ReadFile {
                source: PathBuf::from("/excluded/keep/user-file.txt"),
            },
        ];
        let inputs = ops_to_inputs(ops, &config);
        assert_eq!(inputs.len(), 1);
        match &inputs[0] {
            Input::File(desc) => {
                assert_eq!(desc.path, PathBuf::from("/excluded/keep/user-file.txt"))
            }
            _ => panic!("expected file input"),
        }
    }

    #[test]
    fn test_ops_to_inputs_longest_prefix_re_excludes_leaf() {
        // Re-exclude a leaf inside an exception: `excluded_paths` covers a
        // broad parent, `excluded_path_exceptions` carves out a subdir,
        // and a longer entry in `excluded_paths` re-excludes a leaf inside
        // that subdir. Models the devenv layout: exclude `.devenv/`, keep
        // `.devenv/state/`, but drop devenv-managed `state/tasks.db*`.
        // `Path::starts_with` matches at component boundaries, so each
        // sqlite sibling (`-wal`, `-shm`) is its own component and must be
        // listed explicitly — they are *not* covered by a `tasks.db`
        // prefix.
        let config = CachingConfig {
            excluded_paths: vec![
                PathBuf::from("/d"),
                PathBuf::from("/d/state/tasks.db"),
                PathBuf::from("/d/state/tasks.db-wal"),
                PathBuf::from("/d/state/tasks.db-shm"),
                PathBuf::from("/d/state/git-hooks"),
            ],
            excluded_path_exceptions: vec![PathBuf::from("/d/state")],
            ..Default::default()
        };
        let ops = vec![
            // Dropped by `/d`.
            EvalOp::ReadFile {
                source: PathBuf::from("/d/shell-env.sh"),
            },
            // Kept by `/d/state` carve-out.
            EvalOp::ReadFile {
                source: PathBuf::from("/d/state/postgres/data"),
            },
            // Dropped: leaf exclusions are deeper than the carve-out.
            EvalOp::ReadFile {
                source: PathBuf::from("/d/state/tasks.db"),
            },
            EvalOp::ReadFile {
                source: PathBuf::from("/d/state/tasks.db-wal"),
            },
            EvalOp::ReadFile {
                source: PathBuf::from("/d/state/tasks.db-shm"),
            },
            EvalOp::ReadFile {
                source: PathBuf::from("/d/state/git-hooks/config.json"),
            },
        ];
        let inputs = ops_to_inputs(ops, &config);
        let kept: Vec<_> = inputs
            .iter()
            .map(|i| match i {
                Input::File(d) => d.path.clone(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(kept, vec![PathBuf::from("/d/state/postgres/data")]);
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
}
