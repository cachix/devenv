//! This module provides a SQLite-based implementation for tracking file modifications
//! related to tasks' `exec_if_modified` feature.

use devenv_cache_core::{
    db::Database,
    error::{CacheError, CacheResult},
    file::TrackedFile,
    time,
};
use serde_json::Value;
use sqlx::Row;
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{debug, warn};

// Create a constant for embedded migrations
pub const MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!();

/// Task cache manager
#[derive(Clone, Debug)]
pub struct TaskCache {
    db: Database,
}

impl TaskCache {
    /// Create a new TaskCache using the DEVENV_DOTFILE environment variable.
    pub async fn new() -> CacheResult<Self> {
        let cache_dir = std::env::var("DEVENV_DOTFILE")
            .map_err(|_| CacheError::missing_env_var("DEVENV_DOTFILE"))?;

        // Proper path joining instead of string concatenation
        let mut db_path = PathBuf::from(cache_dir);
        db_path.push("tasks.db");

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

        // Check each file for modifications
        for path in files {
            let modified = self.is_file_modified(task_name, path).await?;
            if modified {
                debug!("File {} has been modified for task {}", path, task_name);
                return Ok(true);
            }
        }

        Ok(false)
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
        let content_hash = tracked_file.content_hash.clone().unwrap_or_default();
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
    async fn update_file_state(&self, task_name: &str, path: &str) -> CacheResult<()> {
        // Use the TrackedFile from the shared library to compute file info
        let tracked_file = TrackedFile::new(path)?;
        self.update_file_state_with_file(task_name, &tracked_file)
            .await
    }

    /// Fetch file information from the database
    async fn fetch_file_info(
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
        // Fetch the existing file info
        let file_info = self.fetch_file_info(task_name, path).await?;

        // If file not in database, consider it modified
        if file_info.is_none() {
            debug!("File {} not found in cache for task {}", path, task_name);
            self.update_file_state(task_name, path).await?;
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
                let current_hash = current_file.content_hash.clone().unwrap_or_default();

                // Combine checking for file type and hash changes
                let content_changed = current_file.is_directory != is_directory
                    || current_hash != stored_hash.unwrap_or_default();

                if content_changed {
                    debug!(
                        "File {} changed for task {}: type or content changed",
                        path, task_name
                    );
                    // Update the file state using the already loaded instance
                    self.update_file_state_with_file(task_name, &current_file)
                        .await?;
                    return Ok(true);
                }

                // If only timestamp changed but hash didn't, update the timestamp without considering it modified
                if current_modified_time > stored_modified_time {
                    debug!(
                        "File {} timestamp changed for task {} but content is the same",
                        path, task_name
                    );
                    // Update using the current file instance we already have
                    self.update_file_state_with_file(task_name, &current_file)
                        .await?;
                }

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
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration};

    #[sqlx::test]
    async fn test_task_cache_initialization() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("DEVENV_DOTFILE", temp_dir.path().to_str().unwrap());

        let cache = TaskCache::new().await.unwrap();
        // Check if the database connection is valid using a simple query
        let result = sqlx::query("SELECT 1").fetch_one(cache.db.pool()).await;
        assert!(result.is_ok());
    }

    #[sqlx::test]
    async fn test_file_modification_detection() {
        let dotfile_dir = TempDir::new().unwrap();
        std::env::set_var("DEVENV_DOTFILE", dotfile_dir.path().to_str().unwrap());

        let cache = TaskCache::new().await.unwrap();
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Create a test file
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"initial content").unwrap();
        }

        let task_name = "test_task";
        let path_str = file_path.to_str().unwrap().to_string();

        // First check should consider it modified (initial run)
        assert!(cache
            .check_modified_files(task_name, &[path_str.clone()])
            .await
            .unwrap());

        // Second check should consider it unmodified
        assert!(!cache
            .check_modified_files(task_name, &[path_str.clone()])
            .await
            .unwrap());

        // More reliable approach to ensure modification time changes:
        // 1. Sleep to ensure system time advances
        // 2. Change content to guarantee hash changes
        // 3. Use multiple write operations to maximize chance of timestamp change

        // Sleep to ensure clock advances, even on platforms with low timestamp resolution
        sleep(Duration::from_millis(10)).await;

        // First update with different content
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"modified content").unwrap();
            file.sync_all().unwrap(); // Force flush to filesystem
        }

        // Sleep again to ensure timestamps can change
        sleep(Duration::from_millis(10)).await;

        // Second write to ensure modification time updates
        {
            // Append more content to guarantee content hash differs
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&file_path)
                .unwrap();
            file.write_all(b" with more text").unwrap();
            file.sync_all().unwrap();
        }

        // Check should detect the modification
        assert!(cache
            .check_modified_files(task_name, &[path_str.clone()])
            .await
            .unwrap());

        // Another check should see it as unmodified again
        assert!(!cache
            .check_modified_files(task_name, &[path_str])
            .await
            .unwrap());
    }
}
