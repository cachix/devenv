//! Caching service for FFI-based Nix evaluation.
//!
//! This module provides transparent caching for `NixRustBackend.eval()` operations.
//! When an eval request is made, the service checks if a valid cached result exists
//! (with unchanged file and env inputs). If so, it returns the cached JSON.
//! Otherwise, the caller performs the actual evaluation and stores the result.
//!
//! ## Transparent Caching Interface
//!
//! The [`CachedEval`] struct provides a transparent caching wrapper that automatically:
//! - Checks cache before evaluation
//! - Collects inputs during evaluation
//! - Stores results after evaluation
//!
//! Callers don't need to know whether caching is enabled - the interface is identical
//! in both cases.

use devenv_cache_core::db::Database;
use futures::future::join_all;
use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, trace, warn};

use crate::db::{self, EnvInputRow, EvalRow, FileInputRow};
use crate::eval_inputs::{
    EnvInputDesc, FileInputDesc, FileState, Input, check_env_state, check_file_state,
};
use crate::ffi_cache::{CachingConfig, EvalCacheKey, EvalInputCollector, ops_to_inputs};
use devenv_core::nix_log_bridge::NixLogBridge;

/// Result of a cache lookup.
#[derive(Debug)]
pub struct CachedEvalResult {
    /// The cached JSON output.
    pub json_output: String,
    /// The eval row ID (for updating timestamps).
    pub eval_id: i64,
}

/// Error type for caching operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Database error: {0}")]
    Database(#[from] turso::Error),
    #[error("Cache core error: {0}")]
    CacheCore(#[from] devenv_cache_core::error::CacheError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Evaluation error: {0}")]
    Eval(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Service for caching eval results.
///
/// This service provides transparent caching for Nix evaluation operations.
/// It validates cached results by checking if input files and environment
/// variables have changed since the result was cached.
pub struct CachingEvalService {
    db: Arc<Database>,
    config: CachingConfig,
}

impl CachingEvalService {
    /// Create a new caching service with the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            config: CachingConfig::default(),
        }
    }

    /// Create a new caching service with custom configuration.
    pub fn with_config(db: Arc<Database>, config: CachingConfig) -> Self {
        Self { db, config }
    }

    /// Get the database reference.
    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Check for a valid cached result.
    ///
    /// Returns `Some(CachedEvalResult)` if a valid cached result exists,
    /// or `None` if the cache is empty or stale.
    pub async fn get_cached(
        &self,
        key: &EvalCacheKey,
    ) -> Result<Option<CachedEvalResult>, CacheError> {
        // Force refresh bypasses cache
        if self.config.force_refresh {
            debug!(key_hash = %key.key_hash, "Force refresh enabled, skipping cache");
            return Ok(None);
        }

        let conn = self.db.connect().await?;

        // Look up the cached eval
        let Some(eval_row) = db::get_eval_by_key_hash(&conn, &key.key_hash).await? else {
            trace!(key_hash = %key.key_hash, "Eval not found in cache");
            return Ok(None);
        };

        // Load file and env inputs
        let file_rows = db::get_files_by_eval_id(&conn, eval_row.id).await?;
        let env_rows = db::get_envs_by_eval_id(&conn, eval_row.id).await?;

        // Validate inputs
        if !self
            .validate_inputs(&eval_row, &file_rows, &env_rows)
            .await?
        {
            debug!(
                key_hash = %key.key_hash,
                attr_name = %key.attr_name,
                "Cached eval invalidated due to input changes"
            );
            return Ok(None);
        }

        // Update timestamp
        db::update_eval_updated_at(&conn, eval_row.id).await?;

        debug!(
            key_hash = %key.key_hash,
            attr_name = %key.attr_name,
            "Cache hit"
        );

        Ok(Some(CachedEvalResult {
            json_output: eval_row.json_output,
            eval_id: eval_row.id,
        }))
    }

    /// Store a new eval result with its inputs.
    pub async fn store(
        &self,
        key: &EvalCacheKey,
        json_output: &str,
        inputs: Vec<Input>,
    ) -> Result<(), CacheError> {
        let conn = self.db.connect().await?;
        let input_hash = Input::compute_input_hash(&inputs);

        db::insert_eval_with_inputs(
            &conn,
            &key.key_hash,
            &key.attr_name,
            &input_hash,
            json_output,
            &inputs,
        )
        .await?;

        debug!(
            key_hash = %key.key_hash,
            attr_name = %key.attr_name,
            num_inputs = inputs.len(),
            "Stored eval result in cache"
        );

        Ok(())
    }

    /// Get the file input paths for a cached eval by key.
    ///
    /// Returns the list of file paths that were tracked during evaluation.
    /// Directories are filtered out since direnv's watch_file only works with files.
    /// Returns an empty vec if the key is not found.
    pub async fn get_file_inputs(
        &self,
        key: &EvalCacheKey,
    ) -> Result<Vec<std::path::PathBuf>, CacheError> {
        let conn = self.db.connect().await?;

        let Some(eval_row) = db::get_eval_by_key_hash(&conn, &key.key_hash).await? else {
            return Ok(Vec::new());
        };

        let file_rows = db::get_files_by_eval_id(&conn, eval_row.id).await?;
        // Filter out directories - direnv watch_file only works with files
        Ok(file_rows
            .into_iter()
            .filter(|r| !r.is_directory)
            .map(|r| r.path)
            .collect())
    }

    /// Validate that cached inputs haven't changed.
    async fn validate_inputs(
        &self,
        eval_row: &EvalRow,
        file_rows: &[FileInputRow],
        env_rows: &[EnvInputRow],
    ) -> Result<bool, CacheError> {
        // Convert rows to input descriptors
        let file_inputs: Vec<Input> = file_rows.iter().map(|r| r.clone().into()).collect();
        let env_inputs: Vec<Input> = env_rows.iter().map(|r| r.clone().into()).collect();

        let mut all_inputs: Vec<Input> = file_inputs;
        all_inputs.extend(env_inputs);

        // Add extra watch paths
        let fallback_time = SystemTime::now();
        for path in &self.config.extra_watch_paths {
            if let Ok(desc) = FileInputDesc::new(path.clone(), fallback_time) {
                all_inputs.push(Input::File(desc));
            }
        }

        // Sort and deduplicate
        all_inputs.sort();
        all_inputs.dedup_by(Input::dedup);

        // Compute new input hash
        let new_input_hash = Input::compute_input_hash(&all_inputs);
        if new_input_hash != eval_row.input_hash {
            trace!(
                cached_hash = %eval_row.input_hash,
                new_hash = %new_input_hash,
                "Input hash mismatch"
            );
            return Ok(false);
        }

        // Check individual file states in parallel
        let file_checks = file_rows
            .iter()
            .map(|row| {
                let desc: FileInputDesc = row.clone().into();
                tokio::task::spawn_blocking(move || check_file_state(&desc))
            })
            .collect::<Vec<_>>();

        let file_results = join_all(file_checks).await;
        for (row, result) in file_rows.iter().zip(file_results) {
            match result {
                Ok(Ok(state)) => match state {
                    FileState::Unchanged | FileState::MetadataModified { .. } => {
                        // File is still valid
                    }
                    FileState::Modified { .. } | FileState::Removed => {
                        trace!(
                            "File '{}' modified or removed, cache invalid",
                            row.path.display()
                        );
                        return Ok(false);
                    }
                },
                Ok(Err(e)) => {
                    trace!(error = %e, "Error checking file state");
                    return Ok(false);
                }
                Err(e) => {
                    trace!(error = %e, "Task join error");
                    return Ok(false);
                }
            }
        }

        // Check env states
        for row in env_rows {
            // Handle empty string → None normalization (empty string in DB means unset)
            let desc = EnvInputDesc {
                name: row.name.clone(),
                content_hash: if row.content_hash.is_empty() {
                    None
                } else {
                    Some(row.content_hash.clone())
                },
            };
            match check_env_state(&desc) {
                Ok(FileState::Unchanged) => {}
                Ok(FileState::Modified { .. } | FileState::Removed) => {
                    trace!("Env var '{}' modified or removed, cache invalid", row.name);
                    return Ok(false);
                }
                Ok(FileState::MetadataModified { .. }) => {
                    // Env vars don't have metadata, this shouldn't happen
                }
                Err(e) => {
                    trace!(error = %e, "Error checking env state");
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }
}

/// Transparent caching wrapper for Nix evaluation.
///
/// This struct provides a unified interface for evaluation that works identically
/// whether caching is enabled or not. Callers simply call `eval()` or `eval_typed()`
/// and the caching layer handles everything transparently:
///
/// - When caching is enabled: checks cache, collects inputs, stores results
/// - When caching is disabled: passes through to evaluation directly
///
/// # Example
///
/// ```ignore
/// // Create with caching
/// let cached_eval = CachedEval::with_cache(service, log_bridge, config);
///
/// // Or without caching (passthrough mode)
/// let cached_eval = CachedEval::without_cache(log_bridge);
///
/// // Same interface either way - caller doesn't need to know
/// let (result, cache_hit) = cached_eval.eval(&key, || async {
///     // Actual evaluation logic
///     Ok(json_string)
/// }).await?;
/// ```
pub struct CachedEval {
    service: Option<CachingEvalService>,
    log_bridge: Arc<NixLogBridge>,
    config: CachingConfig,
}

impl CachedEval {
    /// Create a caching evaluator with caching enabled.
    ///
    /// Evaluation results will be cached and retrieved from the database.
    pub fn with_cache(
        service: CachingEvalService,
        log_bridge: Arc<NixLogBridge>,
        config: CachingConfig,
    ) -> Self {
        Self {
            service: Some(service),
            log_bridge,
            config,
        }
    }

    /// Create a caching evaluator without caching (passthrough mode).
    ///
    /// Evaluation will always run, results won't be cached.
    /// Useful for testing or when caching should be disabled.
    pub fn without_cache(log_bridge: Arc<NixLogBridge>) -> Self {
        Self {
            service: None,
            log_bridge,
            config: CachingConfig::default(),
        }
    }

    /// Check if caching is enabled.
    pub fn is_caching_enabled(&self) -> bool {
        self.service.is_some()
    }

    /// Get a reference to the underlying caching service, if available.
    pub fn service(&self) -> Option<&CachingEvalService> {
        self.service.as_ref()
    }

    /// Get a reference to the log bridge.
    pub fn log_bridge(&self) -> &Arc<NixLogBridge> {
        &self.log_bridge
    }

    /// Evaluate with transparent caching, returning a JSON string.
    ///
    /// The `eval_fn` closure is only called on cache miss. It should perform
    /// the actual Nix evaluation and return the result as a JSON string.
    ///
    /// Returns `(result, cache_hit)` where `cache_hit` indicates whether the
    /// result came from cache.
    ///
    /// # Errors
    ///
    /// Returns `CacheError::Eval` if the evaluation function fails, or
    /// `CacheError::Database` if there's a database error during cache operations.
    pub async fn eval<F, Fut>(
        &self,
        key: &EvalCacheKey,
        eval_fn: F,
    ) -> Result<(String, bool), CacheError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, miette::Error>>,
    {
        let Some(service) = &self.service else {
            // No caching - just evaluate
            let result = eval_fn()
                .await
                .map_err(|e| CacheError::Eval(e.to_string()))?;
            return Ok((result, false));
        };

        // Check cache first
        match service.get_cached(key).await {
            Ok(Some(cached)) => {
                self.log_bridge.mark_cached();
                return Ok((cached.json_output, true));
            }
            Ok(None) => {
                // Cache miss - continue to evaluation
            }
            Err(e) => {
                // Log but don't fail - graceful degradation
                warn!(error = %e, "Cache lookup failed, proceeding with evaluation");
            }
        }

        // Cache miss - collect inputs during evaluation
        let collector = EvalInputCollector::start();
        self.log_bridge.add_observer(collector.clone());

        let result = eval_fn()
            .await
            .map_err(|e| CacheError::Eval(e.to_string()))?;

        // Stop collecting and store result
        self.log_bridge.clear_observers();
        let ops = collector.take_ops();
        let inputs = ops_to_inputs(ops, &self.config);

        if let Err(e) = service.store(key, &result, inputs).await {
            // Log but don't fail - result is still valid
            warn!(error = %e, "Failed to store result in cache");
        }

        Ok((result, false))
    }

    /// Evaluate with transparent caching, deserializing the result to a typed value.
    ///
    /// Similar to `eval()`, but the evaluation function returns a typed value `T`
    /// which is serialized to JSON for caching. On cache hit, the cached JSON
    /// is deserialized back to `T`.
    ///
    /// This is useful when you want to cache structured data (like paths or configs)
    /// rather than raw JSON strings.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The result type, must implement `Serialize` and `DeserializeOwned`
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[derive(Serialize, Deserialize)]
    /// struct ShellPaths {
    ///     drv_path: String,
    ///     out_path: String,
    /// }
    ///
    /// let (paths, cache_hit) = cached_eval.eval_typed::<ShellPaths, _, _>(
    ///     &key,
    ///     || async { Ok(ShellPaths { drv_path, out_path }) },
    /// ).await?;
    /// ```
    pub async fn eval_typed<T, F, Fut>(
        &self,
        key: &EvalCacheKey,
        eval_fn: F,
    ) -> Result<(T, bool), CacheError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, miette::Error>>,
    {
        let Some(service) = &self.service else {
            // No caching - just evaluate
            let result = eval_fn()
                .await
                .map_err(|e| CacheError::Eval(e.to_string()))?;
            return Ok((result, false));
        };

        // Check cache first
        match service.get_cached(key).await {
            Ok(Some(cached)) => {
                self.log_bridge.mark_cached();
                let value: T = serde_json::from_str(&cached.json_output)?;
                return Ok((value, true));
            }
            Ok(None) => {
                // Cache miss - continue to evaluation
            }
            Err(e) => {
                // Log but don't fail - graceful degradation
                warn!(error = %e, "Cache lookup failed, proceeding with evaluation");
            }
        }

        // Cache miss - collect inputs during evaluation
        let collector = EvalInputCollector::start();
        self.log_bridge.add_observer(collector.clone());

        let result = eval_fn()
            .await
            .map_err(|e| CacheError::Eval(e.to_string()))?;

        // Stop collecting and store result
        self.log_bridge.clear_observers();
        let ops = collector.take_ops();
        let inputs = ops_to_inputs(ops, &self.config);

        let json = serde_json::to_string(&result)?;
        if let Err(e) = service.store(key, &json, inputs).await {
            // Log but don't fail - result is still valid
            warn!(error = %e, "Failed to store result in cache");
        }

        Ok((result, false))
    }
}

// =============================================================================
// CachingEvalState Wrapper
// =============================================================================

/// Reason for bypassing the caching layer.
///
/// When accessing `EvalState` directly through `uncached()`, callers must
/// provide a reason. This documents legitimate bypass cases and helps
/// prevent accidental bypasses.
#[derive(Debug, Clone, Copy)]
pub enum UncachedReason {
    /// Lock file validation must check fresh state
    LockValidation,
    /// Interactive REPL has no meaningful caching
    Repl,
    /// Update operation explicitly modifies state
    Update,
    /// Search results are large/dynamic and not worth caching
    Search,
}

/// Wrapper around `EvalState` that enforces caching for all operations.
///
/// This wrapper prevents direct access to `EvalState` methods that could
/// bypass the caching layer. All evaluation paths must go through either:
/// - `eval_to_json()` for cached evaluation
/// - `uncached(reason)` for explicit bypass with documented justification
///
/// The wrapper stores a pre-computed hash of NixArgs for efficient cache
/// key generation during evaluation.
pub struct CachingEvalState<E> {
    /// The underlying eval state (private - not directly accessible)
    eval_state: E,
    /// The caching wrapper
    cached_eval: CachedEval,
    /// Pre-computed serialized NixArgs string for cache key generation
    nix_args_str: String,
}

impl<E> CachingEvalState<E> {
    /// Create a new caching eval state wrapper.
    ///
    /// # Arguments
    /// * `eval_state` - The underlying evaluation state to wrap
    /// * `cached_eval` - The caching service wrapper
    /// * `nix_args_str` - Pre-serialized NixArgs string for cache key generation
    pub fn new(eval_state: E, cached_eval: CachedEval, nix_args_str: String) -> Self {
        Self {
            eval_state,
            cached_eval,
            nix_args_str,
        }
    }

    /// Get a reference to the caching service.
    pub fn cached_eval(&self) -> &CachedEval {
        &self.cached_eval
    }

    /// Get the pre-computed NixArgs string for cache key generation.
    pub fn nix_args_str(&self) -> &str {
        &self.nix_args_str
    }

    /// Create a cache key for the given attribute name.
    pub fn cache_key(&self, attr_name: &str) -> crate::ffi_cache::EvalCacheKey {
        crate::ffi_cache::EvalCacheKey::from_nix_args_str(&self.nix_args_str, attr_name)
    }

    /// Get uncached access for operations that must bypass caching.
    ///
    /// This requires explicitly stating why caching is being bypassed,
    /// preventing accidental bypass of the caching layer.
    ///
    /// # Arguments
    /// * `reason` - Why caching is being bypassed (for documentation/logging)
    ///
    /// # Returns
    /// An `UncachedEvalState` that provides access to the underlying eval state.
    pub fn uncached(&self, reason: UncachedReason) -> UncachedEvalState<'_, E> {
        tracing::debug!(?reason, "Bypassing eval cache");
        UncachedEvalState {
            eval_state: &self.eval_state,
            _reason: reason,
        }
    }

    /// Consume the wrapper and return the underlying eval state.
    ///
    /// This is useful when the wrapper is no longer needed and the
    /// eval state should be used directly.
    pub fn into_inner(self) -> E {
        self.eval_state
    }
}

/// Temporary uncached access to EvalState.
///
/// This is returned by `CachingEvalState::uncached()` and provides
/// limited access to the underlying EvalState for operations that
/// genuinely cannot be cached.
pub struct UncachedEvalState<'a, E> {
    eval_state: &'a E,
    _reason: UncachedReason,
}

impl<'a, E> UncachedEvalState<'a, E> {
    /// Get a reference to the underlying eval state.
    pub fn inner(&self) -> &E {
        self.eval_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MIGRATIONS_PATH;
    use crate::eval_inputs::FileInputDesc;
    use crate::ffi_cache::EvalCacheKey;
    use std::path::Path;
    use std::time::SystemTime;
    use tempfile::TempDir;

    async fn setup_test_db() -> (Arc<Database>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let migrations_path = Path::new(MIGRATIONS_PATH);
        let db = Database::new(db_path, migrations_path).await.unwrap();
        (Arc::new(db), temp_dir)
    }

    #[test]
    fn test_caching_config_default() {
        let config = CachingConfig::default();
        assert!(!config.force_refresh);
        assert!(config.extra_watch_paths.is_empty());
        assert!(config.excluded_paths.is_empty());
    }

    #[tokio::test]
    async fn test_cache_miss_then_hit() {
        let (db, temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.shell");

        // First lookup should be a miss
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());

        // Create a real temp file for the test
        let temp_file = temp_dir.path().join("devenv.nix");
        std::fs::write(&temp_file, "{ }").unwrap();

        // Store a result with the real file
        let json_output = r#"{"shell":"/nix/store/abc-shell"}"#;
        let inputs = vec![Input::File(
            FileInputDesc::new(temp_file, SystemTime::now()).unwrap(),
        )];
        service.store(&key, json_output, inputs).await.unwrap();

        // Second lookup should be a hit
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());
        let cached = result.unwrap();
        assert_eq!(cached.json_output, json_output);
    }

    #[tokio::test]
    async fn test_force_refresh_bypasses_cache() {
        let (db, _temp_dir) = setup_test_db().await;
        let config = CachingConfig {
            force_refresh: true,
            ..Default::default()
        };
        let service = CachingEvalService::with_config(db, config);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.packages");

        // Store a result (no inputs needed for this test)
        let json_output = r#"["pkg1"]"#;
        service.store(&key, json_output, vec![]).await.unwrap();

        // Lookup should still be a miss due to force_refresh
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_with_no_inputs() {
        let (db, _temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.simple");

        // Store with no inputs (simpler test case)
        let json_output = r#"{"value":42}"#;
        service.store(&key, json_output, vec![]).await.unwrap();

        // Should be able to retrieve
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().json_output, json_output);
    }

    #[tokio::test]
    async fn test_cache_update_replaces_previous() {
        let (db, _temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.test");

        // Store first result
        service.store(&key, r#"{"v":1}"#, vec![]).await.unwrap();

        // Store second result with same key
        service.store(&key, r#"{"v":2}"#, vec![]).await.unwrap();

        // Should get the second result
        let result = service.get_cached(&key).await.unwrap().unwrap();
        assert_eq!(result.json_output, r#"{"v":2}"#);
    }

    #[tokio::test]
    async fn test_cache_invalidated_when_file_removed() {
        let (db, temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.file");

        // Create a temp file
        let temp_file = temp_dir.path().join("test.nix");
        std::fs::write(&temp_file, "original content").unwrap();

        // Store with the file as input
        let json_output = r#"{"result":"original"}"#;
        let inputs = vec![Input::File(
            FileInputDesc::new(temp_file.clone(), SystemTime::now()).unwrap(),
        )];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Remove the file
        std::fs::remove_file(&temp_file).unwrap();

        // Cache should now be invalid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidated_when_file_content_changes() {
        let (db, temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.readfile");

        // Create a temp file
        let temp_file = temp_dir.path().join("data.txt");
        std::fs::write(&temp_file, "original content").unwrap();

        // Store with the file as input
        let json_output = r#"{"content":"original content"}"#;
        let inputs = vec![Input::File(
            FileInputDesc::new(temp_file.clone(), SystemTime::now()).unwrap(),
        )];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Modify file content
        std::fs::write(&temp_file, "modified content").unwrap();

        // Set mtime to ensure it's different
        let new_time = SystemTime::now() + std::time::Duration::from_secs(2);
        std::fs::File::open(&temp_file)
            .unwrap()
            .set_modified(new_time)
            .unwrap();

        // Cache should now be invalid due to content change
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_valid_when_file_metadata_changes_but_content_unchanged() {
        let (db, temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.metadata");

        // Create a temp file
        let temp_file = temp_dir.path().join("stable.txt");
        std::fs::write(&temp_file, "stable content").unwrap();

        // Store with the file as input
        let json_output = r#"{"content":"stable content"}"#;
        let inputs = vec![Input::File(
            FileInputDesc::new(temp_file.clone(), SystemTime::now()).unwrap(),
        )];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Touch file (change mtime but not content)
        let new_time = SystemTime::now() + std::time::Duration::from_secs(2);
        std::fs::File::open(&temp_file)
            .unwrap()
            .set_modified(new_time)
            .unwrap();

        // Cache should still be valid (content hash unchanged)
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_cache_with_multiple_file_inputs() {
        let (db, temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.complex");

        // Create multiple temp files
        let file1 = temp_dir.path().join("config.json");
        let file2 = temp_dir.path().join("data.txt");
        std::fs::write(&file1, r#"{"version": "1.0"}"#).unwrap();
        std::fs::write(&file2, "important data").unwrap();

        // Store with multiple file inputs
        let json_output = r#"{"config":{"version":"1.0"},"data":"important data"}"#;
        let inputs = vec![
            Input::File(FileInputDesc::new(file1.clone(), SystemTime::now()).unwrap()),
            Input::File(FileInputDesc::new(file2.clone(), SystemTime::now()).unwrap()),
        ];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Modify only one file
        std::fs::write(&file1, r#"{"version": "2.0"}"#).unwrap();
        let new_time = SystemTime::now() + std::time::Duration::from_secs(2);
        std::fs::File::open(&file1)
            .unwrap()
            .set_modified(new_time)
            .unwrap();

        // Cache should be invalid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_with_env_input() {
        use crate::eval_inputs::EnvInputDesc;

        let (db, _temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.env");

        // Set an environment variable
        let env_name = "TEST_CACHE_ENV_VAR_12345";
        unsafe {
            std::env::set_var(env_name, "test_value");
        }

        // Store with env input
        let json_output = r#"{"env":"test_value"}"#;
        let inputs = vec![Input::Env(EnvInputDesc::new(env_name.to_string()).unwrap())];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Change env var
        unsafe {
            std::env::set_var(env_name, "changed_value");
        }

        // Cache should be invalid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());

        // Clean up
        unsafe {
            std::env::remove_var(env_name);
        }
    }

    #[tokio::test]
    async fn test_cache_invalidated_when_env_removed() {
        use crate::eval_inputs::EnvInputDesc;

        let (db, _temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.envremove");

        // Set an environment variable
        let env_name = "TEST_CACHE_ENV_REMOVE_12345";
        unsafe {
            std::env::set_var(env_name, "value_to_remove");
        }

        // Store with env input
        let json_output = r#"{"env":"value_to_remove"}"#;
        let inputs = vec![Input::Env(EnvInputDesc::new(env_name.to_string()).unwrap())];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Remove env var
        unsafe {
            std::env::remove_var(env_name);
        }

        // Cache should be invalid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_with_mixed_file_and_env_inputs() {
        use crate::eval_inputs::EnvInputDesc;

        let (db, temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.mixed");

        // Create file and set env var
        let temp_file = temp_dir.path().join("mixed.txt");
        std::fs::write(&temp_file, "file content").unwrap();

        let env_name = "TEST_CACHE_MIXED_12345";
        unsafe {
            std::env::set_var(env_name, "env_value");
        }

        // Store with both file and env inputs
        let json_output = r#"{"file":"file content","env":"env_value"}"#;
        let inputs = vec![
            Input::File(FileInputDesc::new(temp_file.clone(), SystemTime::now()).unwrap()),
            Input::Env(EnvInputDesc::new(env_name.to_string()).unwrap()),
        ];
        service.store(&key, json_output, inputs).await.unwrap();

        // Cache should be valid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_some());

        // Change only the env var
        unsafe {
            std::env::set_var(env_name, "changed_env_value");
        }

        // Cache should be invalid
        let result = service.get_cached(&key).await.unwrap();
        assert!(result.is_none());

        // Clean up
        unsafe {
            std::env::remove_var(env_name);
        }
    }

    #[tokio::test]
    async fn test_different_keys_have_separate_caches() {
        let (db, _temp_dir) = setup_test_db().await;
        let service = CachingEvalService::new(db);

        let key1 = EvalCacheKey::from_test_string("(import /test {})", "config.attr1");
        let key2 = EvalCacheKey::from_test_string("(import /test {})", "config.attr2");

        // Store different results for different keys
        service
            .store(&key1, r#"{"attr":"attr1"}"#, vec![])
            .await
            .unwrap();
        service
            .store(&key2, r#"{"attr":"attr2"}"#, vec![])
            .await
            .unwrap();

        // Each key should return its own cached result
        let result1 = service.get_cached(&key1).await.unwrap().unwrap();
        let result2 = service.get_cached(&key2).await.unwrap().unwrap();

        assert_eq!(result1.json_output, r#"{"attr":"attr1"}"#);
        assert_eq!(result2.json_output, r#"{"attr":"attr2"}"#);
    }

    #[tokio::test]
    async fn test_cache_persistence_across_service_instances() {
        let (db, temp_dir) = setup_test_db().await;
        let key = EvalCacheKey::from_test_string("(import /test {})", "config.persist");

        // Create a temp file for the test
        let temp_file = temp_dir.path().join("persist.txt");
        std::fs::write(&temp_file, "persistent content").unwrap();

        // First service instance stores the result
        {
            let service = CachingEvalService::new(db.clone());
            let json_output = r#"{"persistent":true}"#;
            let inputs = vec![Input::File(
                FileInputDesc::new(temp_file.clone(), SystemTime::now()).unwrap(),
            )];
            service.store(&key, json_output, inputs).await.unwrap();
        }

        // Second service instance should find the cached result
        {
            let service = CachingEvalService::new(db);
            let result = service.get_cached(&key).await.unwrap();
            assert!(result.is_some());
            assert_eq!(result.unwrap().json_output, r#"{"persistent":true}"#);
        }
    }
}
