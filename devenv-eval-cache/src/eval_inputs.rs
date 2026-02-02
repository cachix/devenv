//! Input tracking types for Nix evaluation caching.
//!
//! This module contains types that describe file and environment variable
//! dependencies tracked during Nix evaluation for caching purposes.

use devenv_cache_core::{compute_file_hash, compute_string_hash};
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// An input dependency tracked during evaluation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Input {
    File(FileInputDesc),
    Env(EnvInputDesc),
}

impl Input {
    pub fn content_hash(&self) -> Option<&str> {
        match self {
            Self::File(desc) => desc.content_hash.as_deref(),
            Self::Env(desc) => desc.content_hash.as_deref(),
        }
    }

    pub fn compute_input_hash(inputs: &[Self]) -> String {
        compute_string_hash(
            &inputs
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        )
    }

    pub fn partition_refs(inputs: &[Self]) -> (Vec<&FileInputDesc>, Vec<&EnvInputDesc>) {
        let mut file_inputs = Vec::new();
        let mut env_inputs = Vec::new();

        for input in inputs {
            match input {
                Self::File(desc) => file_inputs.push(desc),
                Self::Env(desc) => env_inputs.push(desc),
            }
        }

        (file_inputs, env_inputs)
    }

    pub fn dedup(a: &mut Self, b: &mut Self) -> bool {
        match (a, b) {
            (Input::File(f), Input::File(g)) => {
                f == g
                    || f.path == g.path
                        && f.content_hash == g.content_hash
                        && f.is_directory == g.is_directory
            }
            (Self::Env(f), Self::Env(g)) => f == g,
            _ => false,
        }
    }
}

/// Description of a file input dependency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInputDesc {
    pub path: PathBuf,
    pub is_directory: bool,
    pub content_hash: Option<String>,
    pub modified_at: SystemTime,
}

impl Ord for FileInputDesc {
    /// Sort by path first, then by modified_at in reverse order.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.path.cmp(&other.path) {
            std::cmp::Ordering::Equal => other.modified_at.cmp(&self.modified_at),
            otherwise => otherwise,
        }
    }
}

impl PartialOrd for FileInputDesc {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl FileInputDesc {
    /// Create a new FileInputDesc by reading file metadata and computing hash.
    ///
    /// A fallback system time is required for paths that don't exist.
    /// This avoids duplicate entries for paths that don't exist and would only differ in terms of
    /// the timestamp of when this function was called.
    ///
    /// All timestamps are truncated to second precision.
    pub fn new(path: PathBuf, fallback_system_time: SystemTime) -> Result<Self, io::Error> {
        let is_directory = path.is_dir();
        let content_hash = if is_directory {
            let mut paths: Vec<String> = std::fs::read_dir(&path)?
                .filter_map(Result::ok)
                .map(|entry| entry.path().to_string_lossy().to_string())
                .collect();
            paths.sort();
            Some(compute_string_hash(&paths.join("\n")))
        } else {
            compute_file_hash(&path)
                .map_err(|e| std::io::Error::other(format!("Failed to compute file hash: {e}")))
                .ok()
        };
        let modified_at = truncate_to_seconds(
            path.metadata()
                .and_then(|p| p.modified())
                .unwrap_or(fallback_system_time),
        )?;
        Ok(Self {
            path,
            is_directory,
            content_hash,
            modified_at,
        })
    }
}

/// Description of an environment variable input dependency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvInputDesc {
    pub name: String,
    pub content_hash: Option<String>,
}

impl Ord for EnvInputDesc {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for EnvInputDesc {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl EnvInputDesc {
    /// Create a new EnvInputDesc by reading the current environment variable value.
    pub fn new(name: String) -> Result<Self, io::Error> {
        let value = std::env::var(&name).ok();
        // Normalize: treat empty string same as not present (Nix semantics)
        let content_hash = value
            .filter(|v| !v.is_empty())
            .map(|v| compute_string_hash(&v));
        Ok(Self { name, content_hash })
    }
}

/// Represents the various states of "modified" that we care about.
#[derive(Debug)]
pub enum FileState {
    /// The file has not been modified since it was last cached.
    Unchanged,
    /// The file's metadata, i.e. timestamp, has changed, but its content remains the same.
    MetadataModified { modified_at: SystemTime },
    /// The file's contents have been modified.
    Modified {
        new_hash: String,
        modified_at: SystemTime,
    },
    /// The file no longer exists in the file system.
    Removed,
}

/// Check if a file has changed since it was cached.
pub fn check_file_state(file: &FileInputDesc) -> io::Result<FileState> {
    let metadata = match std::fs::metadata(&file.path) {
        Ok(metadata) => metadata,
        Err(_) => {
            if file.content_hash.is_some() {
                return Ok(FileState::Removed);
            } else {
                return Ok(FileState::Unchanged);
            }
        }
    };

    let modified_at = metadata.modified().and_then(truncate_to_seconds)?;
    if modified_at == file.modified_at {
        // File has not been modified
        return Ok(FileState::Unchanged);
    }

    // mtime has changed, check if content has changed
    let new_hash = if file.is_directory {
        if !metadata.is_dir() {
            return Ok(FileState::Removed);
        }

        let mut paths: Vec<String> = std::fs::read_dir(&file.path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path().to_string_lossy().to_string())
            .collect();
        paths.sort();
        compute_string_hash(&paths.join("\n"))
    } else {
        compute_file_hash(&file.path)
            .map_err(|e| std::io::Error::other(format!("Failed to compute file hash: {e}")))?
    };

    if Some(&new_hash) == file.content_hash.as_ref() {
        // File touched but hash unchanged
        Ok(FileState::MetadataModified { modified_at })
    } else {
        // Hash has changed, return new hash
        Ok(FileState::Modified {
            new_hash,
            modified_at,
        })
    }
}

/// Check if a file's content has changed (ignoring metadata-only changes).
///
/// Returns true if:
/// - File content has changed (different hash)
/// - File was removed
///
/// Returns false if:
/// - File is unchanged
/// - Only metadata changed (e.g., touch without content change)
/// - Error reading file (conservative: assume unchanged)
pub fn has_file_content_changed(file: &FileInputDesc) -> bool {
    match check_file_state(file) {
        Ok(FileState::Unchanged) | Ok(FileState::MetadataModified { .. }) => false,
        Ok(FileState::Modified { .. }) | Ok(FileState::Removed) => true,
        Err(_) => false, // Conservative: treat errors as unchanged
    }
}

/// Check if an environment variable has changed since it was cached.
///
/// Note: In Nix, `builtins.getEnv "FOO"` returns "" for both unset and empty vars.
/// We treat NotPresent and empty string as equivalent to match Nix semantics.
pub fn check_env_state(env: &EnvInputDesc) -> io::Result<FileState> {
    let value = std::env::var(&env.name).ok();

    // Normalize: treat empty string same as not present (Nix semantics)
    let value = value.filter(|v| !v.is_empty());

    // Compute hash of current value (None if not present or empty)
    let current_hash = value.map(|v| compute_string_hash(&v));

    if current_hash == env.content_hash {
        return Ok(FileState::Unchanged);
    }

    // Value changed
    match current_hash {
        Some(new_hash) => Ok(FileState::Modified {
            new_hash,
            modified_at: truncate_to_seconds(SystemTime::now())?,
        }),
        None => Ok(FileState::Removed),
    }
}

/// Truncate a SystemTime to second precision.
///
/// This is useful for comparing timestamps where sub-second precision
/// may cause false mismatches.
pub fn truncate_to_seconds(time: SystemTime) -> io::Result<SystemTime> {
    let duration_since_epoch = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("SystemTime before UNIX EPOCH"))?;
    let seconds = duration_since_epoch.as_secs();
    Ok(UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_file_row(temp_dir: &TempDir, content: &[u8]) -> FileInputDesc {
        let file_path = temp_dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(content).unwrap();
        drop(file);

        let metadata = std::fs::metadata(&file_path).unwrap();
        let modified_at = metadata.modified().unwrap();
        let truncated_modified_at = truncate_to_seconds(modified_at).unwrap();
        let content_hash = compute_file_hash(&file_path).ok();

        FileInputDesc {
            path: file_path,
            is_directory: false,
            content_hash,
            modified_at: truncated_modified_at,
        }
    }

    #[test]
    fn test_unchanged_file() {
        let temp_dir = TempDir::with_prefix("test_unchanged_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        assert!(matches!(
            check_file_state(&file_row),
            Ok(FileState::Unchanged)
        ));
    }

    #[test]
    fn test_metadata_modified_file() {
        let temp_dir = TempDir::with_prefix("test_metadata_modified_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        // Update the file's timestamp to ensure it's different
        let new_time = SystemTime::now() + std::time::Duration::from_secs(1);
        let file = File::open(&file_row.path).unwrap();
        file.set_modified(new_time).unwrap();
        drop(file);

        assert!(matches!(
            check_file_state(&file_row),
            Ok(FileState::MetadataModified { .. })
        ));
    }

    #[test]
    fn test_content_modified_file() {
        let temp_dir = TempDir::with_prefix("test_content_modified_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        // Modify the file contents
        let mut file = File::create(&file_row.path).unwrap();
        file.write_all(b"Modified content").unwrap();

        // Set mtime to ensure it's different from original
        let new_time = SystemTime::now() + std::time::Duration::from_secs(1);
        file.set_modified(new_time).unwrap();

        assert!(matches!(
            check_file_state(&file_row),
            Ok(FileState::Modified { .. })
        ));
    }

    #[test]
    fn test_removed_file() {
        let temp_dir = TempDir::with_prefix("test_removed_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        // Remove the file
        std::fs::remove_file(&file_row.path).unwrap();

        assert!(matches!(
            check_file_state(&file_row),
            Ok(FileState::Removed)
        ));
    }

    #[test]
    fn test_input_dedup_by() {
        let path = PathBuf::from("test.txt");
        let content_hash = Some("abc123".to_string());
        let file1 = Input::File(FileInputDesc {
            path: path.clone(),
            is_directory: false,
            content_hash: content_hash.clone(),
            modified_at: UNIX_EPOCH,
        });
        let file2 = Input::File(FileInputDesc {
            path: path.clone(),
            is_directory: false,
            content_hash: content_hash.clone(),
            modified_at: UNIX_EPOCH + std::time::Duration::from_secs(1),
        });

        let mut inputs = vec![file1, file2.clone()];
        inputs.sort();
        inputs.dedup_by(Input::dedup);
        assert!(inputs.len() == 1);
        assert_eq!(inputs[0], file2);
    }

    #[test]
    fn test_truncate_system_time_to_seconds() {
        let time = SystemTime::now();
        let truncated_time = truncate_to_seconds(time).unwrap();
        let duration_since_epoch = truncated_time.duration_since(UNIX_EPOCH).unwrap();
        // Test that sub-second precision is removed
        assert_eq!(duration_since_epoch.subsec_nanos(), 0);
    }
}
