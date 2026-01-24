//! FFI-based eval caching types for NixRustBackend.
//!
//! This module provides core types for caching evaluation results
//! when using the FFI backend instead of the CLI backend.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
        let key_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();
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

/// Collects input operations during an evaluation scope.
///
/// This collector is registered with `NixLogBridge` before evaluation starts
/// and captures all `EvalOp` events (file reads, env var accesses, etc.) during
/// the evaluation. After evaluation completes, the collected ops are converted
/// to `Input` descriptors for cache storage.
///
/// # Example
///
/// ```ignore
/// let collector = EvalInputCollector::start();
/// log_bridge.add_observer(collector.clone());
///
/// // ... perform evaluation ...
///
/// log_bridge.clear_observers();
/// let inputs = collector.into_inputs(&config);
/// ```
pub struct EvalInputCollector {
    ops: Arc<Mutex<Vec<EvalOp>>>,
    active: Arc<AtomicBool>,
}

impl EvalInputCollector {
    /// Start a new input collector in active state.
    pub fn start() -> Arc<Self> {
        Arc::new(Self {
            ops: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(true)),
        })
    }

    /// Check if the collector is still active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Push an operation to the collector if active.
    ///
    /// This is called by `NixLogBridge` when it detects input operations
    /// during evaluation.
    pub fn push(&self, op: EvalOp) {
        if self.is_active() {
            if let Ok(mut ops) = self.ops.lock() {
                ops.push(op);
            }
        }
    }

    /// Deactivate the collector (stop accepting new ops).
    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
    }

    /// Take all collected operations.
    ///
    /// This consumes the internal ops vector and returns it.
    /// The collector should be stopped before calling this.
    pub fn take_ops(&self) -> Vec<EvalOp> {
        self.stop();
        if let Ok(mut ops) = self.ops.lock() {
            std::mem::take(&mut *ops)
        } else {
            Vec::new()
        }
    }

    /// Convert collected operations to Input descriptors.
    ///
    /// This filters out:
    /// - Paths under `/nix/store` (immutable)
    /// - Paths in `config.excluded_paths`
    /// - Non-absolute paths
    ///
    /// And adds:
    /// - Paths from `config.extra_watch_paths`
    pub fn into_inputs(self: Arc<Self>, config: &CachingConfig) -> Vec<Input> {
        let ops = self.take_ops();
        ops_to_inputs(ops, config)
    }
}

/// Implementation of OpObserver for EvalInputCollector.
///
/// This allows EvalInputCollector to be registered with NixLogBridge as an observer
/// and receive EvalOp events during evaluation.
impl OpObserver for EvalInputCollector {
    fn on_op(&self, eval_op: EvalOp) {
        self.push(eval_op);
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
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
pub fn ops_to_inputs(ops: Vec<EvalOp>, config: &CachingConfig) -> Vec<Input> {
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

                // Skip excluded paths
                if config
                    .excluded_paths
                    .iter()
                    .any(|excluded| source.starts_with(excluded))
                {
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
    fn test_collector_start_is_active() {
        let collector = EvalInputCollector::start();
        assert!(collector.is_active());
    }

    #[test]
    fn test_collector_stop() {
        let collector = EvalInputCollector::start();
        collector.stop();
        assert!(!collector.is_active());
    }

    #[test]
    fn test_collector_push_when_active() {
        let collector = EvalInputCollector::start();
        collector.push(EvalOp::GetEnv {
            name: "FOO".to_string(),
        });
        let ops = collector.take_ops();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn test_collector_push_when_inactive() {
        let collector = EvalInputCollector::start();
        collector.stop();
        collector.push(EvalOp::GetEnv {
            name: "FOO".to_string(),
        });
        let ops = collector.take_ops();
        assert!(ops.is_empty());
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
}
