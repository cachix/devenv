//! This module provides a SQLite-based implementation for tracking file modifications
//! related to tasks' `exec_if_modified` feature.

use devenv_cache_core::{
    db::Database,
    error::{CacheError, CacheResult},
    file::TrackedFile,
    time,
};
use ignore::overrides::OverrideBuilder;
use ignore::Match;
use ignore::WalkBuilder;
use serde_json::Value;
use sqlx::Row;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::{debug, warn};

fn normalize_pattern_for_base_dir(pattern: &str, base_dir: &Path) -> String {
    let (negated, raw_pattern) = match pattern.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };

    if base_dir == Path::new(".") {
        return pattern.to_string();
    }

    let base_dir_str = base_dir.to_string_lossy();
    let stripped = if let Some(rest) = raw_pattern.strip_prefix(&format!("{}/", base_dir_str)) {
        rest
    } else if let Some(rest) = raw_pattern.strip_prefix(&format!("{}\\", base_dir_str)) {
        rest
    } else if raw_pattern == base_dir_str {
        "."
    } else {
        raw_pattern
    };

    if negated {
        format!("!{}", stripped)
    } else {
        stripped.to_string()
    }
}

/// Extract the base directory from a glob pattern.
///
/// Returns the longest path prefix that doesn't contain glob special characters.
/// This is used to determine where to start walking the filesystem.
fn extract_base_dir(pattern: &str) -> &Path {
    // Find the first occurrence of any glob special character
    let special_chars = ['*', '?', '[', '{'];
    let first_special = pattern
        .char_indices()
        .find(|(_, c)| special_chars.contains(c))
        .map(|(i, _)| i)
        .unwrap_or(pattern.len());

    // No glob characters - treat as a literal path and walk from its parent directory
    if first_special == pattern.len() {
        let path = Path::new(pattern);
        if let Some(parent) = path.parent() {
            if parent.as_os_str().is_empty() {
                return Path::new(".");
            }

            return parent;
        }

        return Path::new(".");
    }

    // Get the substring up to the first special character
    let prefix = &pattern[..first_special];

    // Find the last path separator in the prefix to get the directory
    match prefix.rfind(std::path::MAIN_SEPARATOR) {
        Some(last_sep) => Path::new(&pattern[..last_sep]),
        None => {
            // No separator found - use current directory if pattern starts with special char,
            // otherwise the pattern itself might be a file/dir name
            if first_special == 0 {
                Path::new(".")
            } else {
                Path::new(prefix)
            }
        }
    }
}

/// Expand glob patterns into actual file paths
///
/// The expansion walks the filesystem starting from the base directory of each pattern
/// and matches files against the glob patterns.
///
/// Negation patterns (starting with `!`) are supported to exclude paths:
/// - `!**/node_modules/**` – excludes all paths containing node_modules
/// - `!**/*.test.ts` – excludes all test files
///
/// Example: `["**/*.ts", "!**/node_modules/**"]` matches all TypeScript files
/// except those in node_modules directories.
pub fn expand_glob_patterns(patterns: &[String]) -> Vec<String> {
    // Separate positive patterns from negation patterns
    let (negation_patterns, positive_patterns): (Vec<&str>, Vec<&str>) = patterns
        .iter()
        .map(|s| s.as_str())
        .partition(|p| p.starts_with('!'));

    // `exec_if_modified` requires at least one positive pattern.
    if positive_patterns.is_empty() {
        return Vec::new();
    }

    let mut groups: BTreeMap<PathBuf, Vec<&str>> = BTreeMap::new();
    for pattern in &positive_patterns {
        groups
            .entry(extract_base_dir(pattern).to_path_buf())
            .or_default()
            .push(*pattern);
    }

    let mut results: Vec<String> = Vec::new();

    for (base_dir, positive_group) in groups {
        let mut overrides_builder = OverrideBuilder::new(&base_dir);

        for pattern in positive_group {
            let normalized = normalize_pattern_for_base_dir(pattern, &base_dir);
            if let Err(e) = overrides_builder.add(&normalized) {
                warn!("Invalid glob pattern '{}': {}", normalized, e);
            }
        }

        for pattern in &negation_patterns {
            let normalized = normalize_pattern_for_base_dir(pattern, &base_dir);
            if let Err(e) = overrides_builder.add(&normalized) {
                warn!("Invalid glob pattern '{}': {}", normalized, e);
            }
        }

        let overrides = match overrides_builder.build() {
            Ok(overrides) => overrides,
            Err(e) => {
                warn!("Failed to build ignore overrides: {}", e);
                continue;
            }
        };

        let mut walk_builder = WalkBuilder::new(&base_dir);
        walk_builder
            .hidden(false)
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .parents(false)
            .follow_links(true);

        for entry in walk_builder.build() {
            let Ok(entry) = entry else {
                continue;
            };

            let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
            if matches!(overrides.matched(entry.path(), is_dir), Match::Whitelist(_)) {
                results.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }

    results
}

// Create a constant for embedded migrations
pub const MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!();

/// Task cache manager
#[derive(Clone, Debug)]
pub struct TaskCache {
    db: Database,
}

impl TaskCache {
    /// Create a new TaskCache with the given cache directory.
    pub async fn new(cache_dir: &Path) -> CacheResult<Self> {
        let db_path = cache_dir.join("tasks.db");
        Self::with_db_path(db_path).await
    }

    /// Create a new TaskCache with a specific database path.
    pub async fn with_db_path(db_path: PathBuf) -> CacheResult<Self> {
        // Connect to database using the shared database library and run migrations
        let db = Database::new(db_path, &MIGRATIONS).await?;

        Ok(Self { db })
    }

    /// Get the database connection pool
    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.db.pool()
    }

    // Remove the generic execute_query method as it's causing type issues

    /// Check if any files have been modified for a given task.
    ///
    /// Returns true if any of the files have been modified since the last time
    /// the task was run, or if this is the first time checking these files.
    pub async fn check_modified_files(
        &self,
        task_name: &str,
        files: &[String],
    ) -> CacheResult<bool> {
        if files.is_empty() {
            return Ok(false);
        }

        // Expand all patterns and collect results
        let expanded_paths = expand_glob_patterns(files);

        // Check all files and track if any are modified
        // Important: We need to check ALL files, not return early,
        // so that all files get recorded in the database
        let mut any_modified = false;
        for path in &expanded_paths {
            let modified = self.is_file_modified(task_name, path).await?;
            if modified {
                any_modified = true;
                // Continue checking other files instead of returning early
            }
        }

        Ok(any_modified)
    }

    /// Get current Unix timestamp
    fn now() -> i64 {
        time::system_time_to_unix_seconds(SystemTime::now())
    }

    /// Store task output in the cache.
    pub async fn store_task_output(&self, task_name: &str, output: &Value) -> CacheResult<()> {
        let output_json = serde_json::to_string(output)?;
        let now = Self::now();

        sqlx::query(
            r#"
            INSERT INTO task_run (task_name, last_run, output)
            VALUES (?, ?, ?)
            ON CONFLICT (task_name) DO UPDATE SET
                last_run = excluded.last_run,
                output = excluded.output
            "#,
        )
        .bind(task_name)
        .bind(now)
        .bind(output_json)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    /// Get task output from the cache.
    pub async fn get_task_output(&self, task_name: &str) -> CacheResult<Option<Value>> {
        let result: Option<String> = sqlx::query_scalar(
            r#"
            SELECT output FROM task_run WHERE task_name = ?
            "#,
        )
        .bind(task_name)
        .fetch_optional(self.pool())
        .await?;

        match result {
            Some(json_str) => Ok(serde_json::from_str(&json_str)?),
            None => Ok(None),
        }
    }

    /// Update the file state in the database.
    async fn update_file_state_with_file(
        &self,
        task_name: &str,
        tracked_file: &TrackedFile,
    ) -> CacheResult<()> {
        let path_str = tracked_file.path.to_str().unwrap_or("");
        let is_directory = tracked_file.is_directory;
        let content_hash = tracked_file.content_hash.clone();
        let modified_time = time::system_time_to_unix_seconds(tracked_file.modified_at);

        sqlx::query(
            r#"
            INSERT INTO watched_file (task_name, path, modified_time, content_hash, is_directory)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT (task_name, path) DO UPDATE SET
                modified_time = excluded.modified_time,
                content_hash = excluded.content_hash,
                is_directory = excluded.is_directory
            "#,
        )
        .bind(task_name)
        .bind(path_str)
        .bind(modified_time)
        .bind(content_hash)
        .bind(is_directory)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    /// Update the file state in the database.
    pub async fn update_file_state(&self, task_name: &str, path: &str) -> CacheResult<()> {
        debug!(
            "Updating file state for task '{}', path '{}'",
            task_name, path
        );
        // Use the TrackedFile from the shared library to compute file info
        let tracked_file = TrackedFile::new(path)?;
        self.update_file_state_with_file(task_name, &tracked_file)
            .await
    }

    /// Fetch file information from the database
    pub async fn fetch_file_info(
        &self,
        task_name: &str,
        path: &str,
    ) -> CacheResult<Option<sqlx::sqlite::SqliteRow>> {
        sqlx::query(
            r#"
            SELECT modified_time, content_hash, is_directory
            FROM watched_file
            WHERE task_name = ? AND path = ?
            "#,
        )
        .bind(task_name)
        .bind(path)
        .fetch_optional(self.pool())
        .await
        .map_err(CacheError::from)
    }

    /// Check if a file has been modified since the last time the task was run.
    async fn is_file_modified(&self, task_name: &str, path: &str) -> CacheResult<bool> {
        debug!(
            "Checking if file '{}' is modified for task '{}'",
            path, task_name
        );

        // Fetch the existing file info
        let file_info = self.fetch_file_info(task_name, path).await?;

        // If file not in database, consider it modified (first run)
        if file_info.is_none() {
            debug!(
                "File {} not found in cache for task {} - considering it modified (first time)",
                path, task_name
            );
            // Don't update file state here - only update after successful task completion
            return Ok(true);
        }

        // Check if file exists and get its current state
        match TrackedFile::new(path) {
            Ok(current_file) => {
                let row = file_info.unwrap();

                // Extract values from the row
                let stored_modified_time: i64 = row.get("modified_time");
                let stored_hash: Option<String> = row.get("content_hash");
                let is_directory: bool = row.get("is_directory");

                // Get current values
                let current_modified_time =
                    time::system_time_to_unix_seconds(current_file.modified_at);
                let current_hash = current_file.content_hash.clone();

                debug!(
                    "File '{}' for task '{}': stored_hash={:?}, current_hash={:?}, stored_time={}, current_time={}, is_dir={}",
                    path,
                    task_name,
                    stored_hash,
                    current_hash,
                    stored_modified_time,
                    current_modified_time,
                    is_directory
                );

                // Combine checking for file type and hash changes
                let content_changed =
                    current_file.is_directory != is_directory || current_hash != stored_hash;

                if content_changed {
                    debug!(
                        "File {} changed for task {}: type or content changed (is_dir: {} -> {}, hash: {:?} -> {:?})",
                        path,
                        task_name,
                        is_directory,
                        current_file.is_directory,
                        stored_hash,
                        current_hash
                    );
                    // Don't update file state here - only update after successful task completion
                    return Ok(true);
                }

                // If only timestamp changed but hash didn't, we can update the timestamp
                // since it doesn't affect caching logic (content hash is the same)
                if current_modified_time > stored_modified_time {
                    debug!(
                        "File {} timestamp changed for task {} but content is the same (time: {} -> {})",
                        path, task_name, stored_modified_time, current_modified_time
                    );
                    // Update timestamp only - this is safe since content hash didn't change
                    self.update_file_state_with_file(task_name, &current_file)
                        .await?;
                }

                debug!("File '{}' for task '{}' is unchanged", path, task_name);
                Ok(false)
            }
            Err(e) => {
                warn!("Failed to check file {}: {}", path, e);
                // File doesn't exist or is inaccessible, consider unchanged
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    #[sqlx::test]
    async fn test_task_cache_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("tasks.db");

        // Use with_db_path directly instead of environment variable
        let cache = TaskCache::with_db_path(db_path).await.unwrap();

        // Check if the database connection is valid using a simple query
        let result = sqlx::query("SELECT 1").fetch_one(cache.db.pool()).await;
        assert!(result.is_ok());
    }

    #[sqlx::test]
    async fn test_file_modification_detection() {
        let db_temp_dir = TempDir::new().unwrap();
        let db_path = db_temp_dir.path().join("tasks-file-mod.db");

        let cache = TaskCache::with_db_path(db_path).await.unwrap();
        let test_temp_dir = TempDir::new().unwrap();
        let file_path = test_temp_dir.path().join("test.txt");

        // Create a test file
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"initial content").await.unwrap();
            file.sync_all().await.unwrap(); // Ensure file is fully written to disk
        }

        let task_name = "test_task";
        let path_str = file_path.to_str().unwrap().to_string();

        // First check should consider it modified (initial run)
        assert!(
            cache
                .check_modified_files(task_name, &[path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache.update_file_state(task_name, &path_str).await.unwrap();

        // File now in cache, unchanged
        assert!(
            !cache
                .check_modified_files(task_name, &[path_str.clone()])
                .await
                .unwrap()
        );

        // More reliable approach to ensure modification time changes:
        // 1. Sleep to ensure system time advances
        // 2. Change content to guarantee hash changes
        // 3. Use multiple write operations to maximize chance of timestamp change

        // Modify file content and set mtime to ensure detection
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"modified content with more text")
                .await
                .unwrap();
            file.sync_all().await.unwrap(); // Force flush to filesystem

            // Set mtime to ensure it's different from original
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the modification
        assert!(
            cache
                .check_modified_files(task_name, &[path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache.update_file_state(task_name, &path_str).await.unwrap();

        // Another check should see it as unmodified again
        assert!(
            !cache
                .check_modified_files(task_name, &[path_str])
                .await
                .unwrap()
        );
    }

    #[sqlx::test]
    async fn test_glob_pattern_support() {
        let db_temp_dir = TempDir::new().unwrap();
        let db_path = db_temp_dir.path().join("tasks-glob.db");

        let cache = TaskCache::with_db_path(db_path).await.unwrap();
        let test_temp_dir = TempDir::new().unwrap();

        // Create test files
        let file1_path = test_temp_dir.path().join("test1.txt");
        let file2_path = test_temp_dir.path().join("test2.txt");
        let file3_path = test_temp_dir.path().join("other.log");

        {
            let mut file1 = File::create(&file1_path).await.unwrap();
            file1.write_all(b"content1").await.unwrap();
            file1.sync_all().await.unwrap();

            let mut file2 = File::create(&file2_path).await.unwrap();
            file2.write_all(b"content2").await.unwrap();
            file2.sync_all().await.unwrap();

            let mut file3 = File::create(&file3_path).await.unwrap();
            file3.write_all(b"content3").await.unwrap();
            file3.sync_all().await.unwrap();
        }

        let task_name = "test_glob_task";
        let pattern = format!("{}/*.txt", test_temp_dir.path().to_str().unwrap());

        // First check should consider files modified (initial run)
        assert!(
            cache
                .check_modified_files(task_name, &[pattern.clone()])
                .await
                .unwrap()
        );

        // Store file state in caches
        cache
            .update_file_state(task_name, file1_path.to_str().unwrap())
            .await
            .unwrap();
        cache
            .update_file_state(task_name, file2_path.to_str().unwrap())
            .await
            .unwrap();

        // Second check should consider them unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[pattern.clone()])
                .await
                .unwrap()
        );

        // Modify one of the matched files
        {
            let mut file1 = File::create(&file1_path).await.unwrap();
            file1.write_all(b"modified content1").await.unwrap();
            file1.sync_all().await.unwrap();

            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
            file1.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the modification
        assert!(
            cache
                .check_modified_files(task_name, &[pattern.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, file1_path.to_str().unwrap())
            .await
            .unwrap();

        // Test with multiple patterns
        let pattern2 = format!("{}/*.log", test_temp_dir.path().to_str().unwrap());
        let patterns = vec![pattern.clone(), pattern2];

        // First check with new pattern should detect the .log file as modified (not tracked before)
        assert!(
            cache
                .check_modified_files(task_name, &patterns)
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, file3_path.to_str().unwrap())
            .await
            .unwrap();

        // Second check should consider all files unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &patterns)
                .await
                .unwrap()
        );

        // Modify the .log file
        {
            let mut file3 = File::create(&file3_path).await.unwrap();
            file3.write_all(b"modified log content").await.unwrap();
            file3.sync_all().await.unwrap();

            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
            file3.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the modification in .log file
        assert!(
            cache
                .check_modified_files(task_name, &patterns)
                .await
                .unwrap()
        );
    }

    #[sqlx::test]
    async fn test_directory_modification_detection() {
        let db_temp_dir = TempDir::new().unwrap();
        let db_path = db_temp_dir.path().join("tasks-dir-mod.db");

        let cache = TaskCache::with_db_path(db_path).await.unwrap();
        let test_temp_dir = TempDir::new().unwrap();
        let dir_path = test_temp_dir.path().join("test_dir");
        tokio::fs::create_dir(&dir_path).await.unwrap();

        let task_name = "test_task_dir";
        let dir_path_str = dir_path.to_str().unwrap().to_string();

        // First check should consider it modified (initial run)
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Second check should consider it unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Add a new file in the directory
        let file_path = dir_path.join("test_file.txt");
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"new file content").await.unwrap();
            file.sync_all().await.unwrap();

            // Set mtime to ensure directory modification is detected
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the directory modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Second check should consider it unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Modify an existing file
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"modified file content").await.unwrap();
            file.sync_all().await.unwrap();

            // Set mtime to ensure directory modification is detected
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the directory modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Create a subdirectory and set its mtime
        let subdir_path = dir_path.join("subdir");
        tokio::fs::create_dir(&subdir_path).await.unwrap();

        // Set subdirectory mtime to ensure detection
        let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3);
        File::open(&subdir_path)
            .await
            .unwrap()
            .into_std()
            .await
            .set_modified(new_time)
            .unwrap();

        // Check should detect the directory modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Add a file in the subdirectory
        let subdir_file_path = subdir_path.join("nested_file.txt");
        {
            let mut file = File::create(&subdir_file_path).await.unwrap();
            file.write_all(b"nested file content").await.unwrap();
            file.sync_all().await.unwrap();

            // Set mtime to ensure directory modification is detected
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(4);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the directory modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // After update, it should be unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Create a deeply nested directory structure
        let deep_dir1 = subdir_path.join("level1");
        tokio::fs::create_dir(&deep_dir1).await.unwrap();
        let deep_dir2 = deep_dir1.join("level2");
        tokio::fs::create_dir(&deep_dir2).await.unwrap();
        let deep_dir3 = deep_dir2.join("level3");
        tokio::fs::create_dir(&deep_dir3).await.unwrap();

        // Check should detect the deep directory modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // After update, it should be unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Add a file deep in the nested structure
        let deep_file_path = deep_dir3.join("deep_file.txt");
        {
            let mut file = File::create(&deep_file_path).await.unwrap();
            file.write_all(b"deep nested file content").await.unwrap();
            file.sync_all().await.unwrap();

            // Set mtime to ensure directory modification is detected
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the deep file modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Update the deep file
        {
            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&deep_file_path)
                .await
                .unwrap();
            file.write_all(b" with additional content").await.unwrap();
            file.sync_all().await.unwrap();

            // Set mtime to ensure directory modification is detected
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(6);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the deep file update
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Remove a deep file
        tokio::fs::remove_file(&deep_file_path).await.unwrap();

        // Check should detect the removal
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // Remove a deep directory
        tokio::fs::remove_dir(&deep_dir3).await.unwrap();

        // Check should detect the directory removal
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Store file state in cache
        cache
            .update_file_state(task_name, &dir_path_str)
            .await
            .unwrap();

        // After update, it should be unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str])
                .await
                .unwrap()
        );
    }

    #[test]
    fn test_expand_glob_patterns_with_negation() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create directory structure:
        // base/
        //   src/
        //     main.ts
        //     util.ts
        //   node_modules/
        //     package/
        //       index.ts
        //   test.ts

        let src_dir = base.join("src");
        let node_modules_dir = base.join("node_modules");
        let package_dir = node_modules_dir.join("package");

        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&package_dir).unwrap();

        std::fs::write(src_dir.join("main.ts"), "main").unwrap();
        std::fs::write(src_dir.join("util.ts"), "util").unwrap();
        std::fs::write(package_dir.join("index.ts"), "index").unwrap();
        std::fs::write(base.join("test.ts"), "test").unwrap();

        // Test: all .ts files without negation
        let pattern = format!("{}/**/*.ts", base.display());
        let all_files = expand_glob_patterns(&[pattern.clone()]);
        assert_eq!(all_files.len(), 4); // main.ts, util.ts, index.ts, test.ts (via **)

        // Test: exclude node_modules
        let negation = "!**/node_modules/**".to_string();
        let filtered = expand_glob_patterns(&[pattern.clone(), negation]);
        assert_eq!(filtered.len(), 3); // main.ts, util.ts, test.ts (excludes index.ts)
        assert!(filtered.iter().all(|p| !p.contains("node_modules")));

        // Test: multiple negation patterns
        let negation1 = "!**/node_modules/**".to_string();
        let negation2 = "!**/test.ts".to_string();
        let filtered2 = expand_glob_patterns(&[pattern.clone(), negation1, negation2]);
        assert_eq!(filtered2.len(), 2); // main.ts, util.ts only
        assert!(filtered2.iter().all(|p| !p.contains("node_modules")));
        assert!(filtered2.iter().all(|p| !p.ends_with("test.ts")));
    }

    #[test]
    fn test_expand_glob_patterns_negation_only() {
        // Test that negation-only patterns return empty (no positive patterns to match)
        let result = expand_glob_patterns(&["!**/node_modules/**".to_string()]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_expand_glob_patterns_empty() {
        let result = expand_glob_patterns(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_base_dir() {
        // Pattern with base directory
        assert_eq!(extract_base_dir("/foo/bar/**/*.ts"), Path::new("/foo/bar"));
        assert_eq!(
            extract_base_dir("/home/user/src/**/*.rs"),
            Path::new("/home/user/src")
        );

        // Pattern starting with glob
        assert_eq!(extract_base_dir("**/*.ts"), Path::new("."));
        assert_eq!(extract_base_dir("*.ts"), Path::new("."));

        // Pattern with no glob - returns directory part since no separator after the prefix
        assert_eq!(extract_base_dir("/foo/bar/file.ts"), Path::new("/foo/bar"));

        // Pattern with glob in middle
        assert_eq!(extract_base_dir("/foo/*/bar/*.ts"), Path::new("/foo"));
    }
}
