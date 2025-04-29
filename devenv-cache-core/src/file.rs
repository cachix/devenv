use crate::error::{CacheError, CacheResult};
use crate::time;
use blake3::Hasher;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Represents a file that's being tracked for changes
#[derive(Debug, Clone)]
pub struct TrackedFile {
    /// Path to the file
    pub path: PathBuf,
    /// Whether the path is a directory
    pub is_directory: bool,
    /// Content hash of the file (or directory)
    pub content_hash: Option<String>,
    /// Last modified time
    pub modified_at: SystemTime,
    /// When this file was last checked
    pub checked_at: SystemTime,
}

/// Get file metadata with consistent error handling
fn get_metadata<P: AsRef<Path>>(path: P) -> CacheResult<std::fs::Metadata> {
    let path = path.as_ref();
    std::fs::metadata(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            CacheError::FileNotFound(path.to_path_buf())
        } else {
            e.into()
        }
    })
}

impl TrackedFile {
    /// Create a new TrackedFile from a path
    pub fn new<P: AsRef<Path>>(path: P) -> CacheResult<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = get_metadata(&path)?;

        let is_directory = metadata.is_dir();
        let modified_at = metadata.modified().map_err(|e| CacheError::HashFailure {
            path: path.clone(),
            reason: format!("Failed to get modification time: {}", e),
        })?;

        let content_hash = if is_directory {
            compute_directory_hash(&path)?
        } else {
            Some(compute_file_hash(&path)?)
        };

        Ok(Self {
            path,
            is_directory,
            content_hash,
            modified_at,
            checked_at: SystemTime::now(),
        })
    }

    /// Check if this file has been modified since it was last tracked
    pub fn is_modified(&self) -> CacheResult<bool> {
        // Get current file state
        let current = TrackedFile::new(&self.path)?;

        // Quick check: if content hashes are different, the file has changed
        if current.content_hash != self.content_hash {
            return Ok(true);
        }

        // Check if the file type changed (directory vs file)
        if current.is_directory != self.is_directory {
            return Ok(true);
        }

        // If modification time hasn't changed, file definitely hasn't changed
        if current.modified_at <= self.modified_at {
            return Ok(false);
        }

        // Modification time changed but hashes are the same:
        // This can happen when a file is touched or saved without changes
        // Return false as the file content hasn't actually changed
        Ok(false)
    }

    /// Update the file's content hash and modification time
    pub fn update(&mut self) -> CacheResult<()> {
        let current = TrackedFile::new(&self.path)?;
        self.content_hash = current.content_hash;
        self.modified_at = current.modified_at;
        self.checked_at = SystemTime::now();
        Ok(())
    }

    /// Get the content hash of the file
    pub fn hash(&self) -> Option<&str> {
        self.content_hash.as_deref()
    }

    /// Get the modified time as Unix seconds
    pub fn modified_time(&self) -> i64 {
        time::system_time_to_unix_seconds(self.modified_at)
    }

    /// Convert to a database-friendly representation
    pub fn to_db_values(&self) -> (PathBuf, bool, Option<String>, i64, i64) {
        (
            self.path.clone(),
            self.is_directory,
            self.content_hash.clone(),
            self.modified_time(),
            time::system_time_to_unix_seconds(self.checked_at),
        )
    }
}

/// Helper to open a file with consistent error handling
fn open_file<P: AsRef<Path>>(path: P) -> CacheResult<std::fs::File> {
    let path = path.as_ref();
    std::fs::File::open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            CacheError::FileNotFound(path.to_path_buf())
        } else {
            e.into()
        }
    })
}

/// Compute a hash of a file's contents
pub fn compute_file_hash<P: AsRef<Path>>(path: P) -> CacheResult<String> {
    let path = path.as_ref();
    let mut file = open_file(path)?;
    let mut hasher = Hasher::new();

    io::copy(&mut file, &mut hasher).map_err(|e| CacheError::HashFailure {
        path: path.to_path_buf(),
        reason: format!("Failed to read file: {}", e),
    })?;

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute a hash of a directory's contents
fn compute_directory_hash<P: AsRef<Path>>(path: P) -> CacheResult<Option<String>> {
    let path = path.as_ref();
    let mut entries = Vec::new();

    // Skip the root directory itself, sort by file name for consistent ordering
    for entry in WalkDir::new(path).min_depth(1).sort_by_file_name() {
        match entry {
            Ok(entry) => {
                let entry_path = entry.path().to_string_lossy().into_owned();
                let meta = entry.metadata();

                if let Ok(meta) = meta {
                    let entry_type = if meta.is_dir() { "dir" } else { "file" };
                    let modified = meta
                        .modified()
                        .map(|t| time::system_time_to_unix_seconds(t))
                        .unwrap_or(0);

                    entries.push(format!("{} {} {}", entry_type, modified, entry_path));

                    // For files, also include content hash for maximum detection sensitivity
                    if meta.is_file() {
                        match compute_file_hash(entry.path()) {
                            Ok(hash) => entries.push(format!("hash {}", hash)),
                            Err(_) => entries.push(format!("hash_error {}", entry_path)),
                        }
                    }
                } else {
                    // Fall back to just the path if metadata is unavailable
                    entries.push(entry_path);
                }
            }
            Err(e) => {
                // Include error entries as well to detect when errors change
                entries.push(format!("error {}", e));
            }
        }
    }

    if entries.is_empty() {
        return Ok(None);
    }

    Ok(Some(compute_string_hash(&entries.join("\n"))))
}

/// Compute a hash of a string
pub fn compute_string_hash(content: &str) -> String {
    let hash = blake3::hash(content.as_bytes());
    hash.to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_file_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Create test file
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"test content").unwrap();
        }

        let hash = compute_file_hash(&file_path).unwrap();
        assert!(!hash.is_empty());

        // Same content should produce same hash
        let hash2 = compute_file_hash(&file_path).unwrap();
        assert_eq!(hash, hash2);

        // Different content should produce different hash
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"different content").unwrap();
        }

        let hash3 = compute_file_hash(&file_path).unwrap();
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_tracked_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("tracked.txt");

        // Create test file
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"initial content").unwrap();
        }

        // Track the file
        let tracked = TrackedFile::new(&file_path).unwrap();
        assert!(!tracked.is_directory);
        assert!(tracked.content_hash.is_some());

        // Modification check should return false for unmodified file
        assert!(!tracked.is_modified().unwrap());

        // Modify the file
        std::thread::sleep(std::time::Duration::from_millis(10)); // Ensure modification time changes
        {
            let mut file = File::create(&file_path).unwrap();
            file.write_all(b"modified content").unwrap();
        }

        // Modification check should now return true
        assert!(tracked.is_modified().unwrap());

        // Update should refresh the hash and timestamps
        let mut updated = tracked.clone();
        updated.update().unwrap();
        assert_ne!(tracked.content_hash, updated.content_hash);
        assert_ne!(tracked.modified_at, updated.modified_at);

        // After update, modification check should return false again
        assert!(!updated.is_modified().unwrap());
    }
}
