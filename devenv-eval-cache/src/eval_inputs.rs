//! Input tracking types for Nix evaluation caching.
//!
//! This module contains types that describe file and environment variable
//! dependencies tracked during Nix evaluation for caching purposes.

use devenv_cache_core::{
    compute_directory_content_hash, compute_file_hash, compute_source_file_hash,
    compute_string_hash,
};
use std::io;
use std::path::{Path, PathBuf};
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
                .collect::<Vec<_>>()
                .join("\0"),
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
                        && f.recursive == g.recursive
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
    /// Whether this path is copied into the Nix store.
    ///
    /// For directories, copied paths are hashed recursively over their contents.
    /// For files, copied paths include the owner's executable bit in their hash.
    /// `false` inputs retain the cheaper observation-specific hashing used by
    /// operations such as `readFile` and `readDir`.
    pub recursive: bool,
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
    pub fn new(
        path: PathBuf,
        fallback_system_time: SystemTime,
        recursive: bool,
    ) -> Result<Self, io::Error> {
        let is_directory = path.is_dir();
        let content_hash = if is_directory {
            Some(hash_directory(&path, recursive)?)
        } else if recursive {
            compute_source_file_hash(&path)
                .map_err(|e| std::io::Error::other(format!("Failed to hash source file: {e}")))
                .ok()
        } else {
            compute_path_hash(&path, false).ok()
        };
        let modified_at = truncate_to_seconds(
            path.metadata()
                .and_then(|p| p.modified())
                .unwrap_or(fallback_system_time),
        )?;
        Ok(Self {
            path,
            is_directory,
            recursive,
            content_hash,
            modified_at,
        })
    }
}

/// Hash a directory's contents.
///
/// When `recursive` is set, the entire tree's file contents are hashed (used for
/// sources copied into the Nix store). Otherwise only the sorted list of
/// immediate entry names is hashed (used for `readDir`, where only the listing
/// is observed during evaluation).
fn hash_directory(path: &std::path::Path, recursive: bool) -> Result<String, io::Error> {
    if recursive {
        compute_directory_content_hash(path).map_err(|e| {
            std::io::Error::other(format!("Failed to compute directory content hash: {e}"))
        })
    } else {
        let mut paths: Vec<String> = std::fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path().to_string_lossy().to_string())
            .collect();
        paths.sort();
        Ok(compute_string_hash(&paths.join("\n")))
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

/// Compute the content hash for a path.
///
/// Directories hash their immediate entry names; files hash their contents.
fn compute_path_hash(path: &Path, is_directory: bool) -> io::Result<String> {
    if is_directory {
        let mut paths: Vec<String> = std::fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path().to_string_lossy().to_string())
            .collect();
        paths.sort();
        Ok(compute_string_hash(&paths.join("\n")))
    } else {
        compute_file_hash(path)
            .map_err(|e| io::Error::other(format!("Failed to compute file hash: {e}")))
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
    // The path had no content hash when cached: it did not exist (its
    // modified_at is a fallback snapshot timestamp, not a real mtime) or it
    // could not be hashed. It is readable now, so treat it as modified. The
    // mtime equality check below would mask a creation that happened in the
    // same second as the snapshot.
    if file.content_hash.is_none() {
        let new_hash = if metadata.is_dir() {
            hash_directory(&file.path, file.recursive)?
        } else if file.recursive {
            compute_source_file_hash(&file.path)
                .map_err(|e| io::Error::other(format!("Failed to hash source file: {e}")))?
        } else {
            compute_path_hash(&file.path, false)?
        };
        return Ok(FileState::Modified {
            new_hash,
            modified_at,
        });
    }

    let mtime_unchanged = modified_at == file.modified_at;

    // Copied paths cannot use the mtime fast path: nested edits do not update a
    // directory's mtime, and chmod does not update a regular file's mtime.
    let needs_rehash = file.recursive;
    if mtime_unchanged && !needs_rehash {
        return Ok(FileState::Unchanged);
    }

    // Recompute the hash to see whether the content has actually changed.
    let new_hash = if file.is_directory {
        if !metadata.is_dir() {
            return Ok(FileState::Removed);
        }
        hash_directory(&file.path, file.recursive)?
    } else {
        let hash = if file.recursive {
            compute_source_file_hash(&file.path)
        } else {
            compute_file_hash(&file.path)
        };
        hash.map_err(|e| std::io::Error::other(format!("Failed to compute file hash: {e}")))?
    };

    if Some(&new_hash) == file.content_hash.as_ref() {
        if mtime_unchanged {
            // Re-hashed a recursive directory and nothing changed.
            Ok(FileState::Unchanged)
        } else {
            // Touched but hash unchanged.
            Ok(FileState::MetadataModified { modified_at })
        }
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
    use std::os::unix::fs::PermissionsExt;
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
            recursive: false,
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
    fn test_created_file_with_colliding_mtime_is_modified() {
        // A path that was missing when cached is tracked with a fallback
        // timestamp instead of a real mtime. A file created in the same
        // second as that timestamp must still be detected as modified.
        let temp_dir = TempDir::with_prefix("test_created_file").unwrap();
        let file_path = temp_dir.path().join("appeared.txt");
        std::fs::write(&file_path, b"now I exist").unwrap();
        let mtime = truncate_to_seconds(std::fs::metadata(&file_path).unwrap().modified().unwrap())
            .unwrap();

        let missing_row = FileInputDesc {
            path: file_path,
            is_directory: false,
            recursive: false,
            content_hash: None,
            modified_at: mtime,
        };

        assert!(matches!(
            check_file_state(&missing_row),
            Ok(FileState::Modified { .. })
        ));
    }

    #[test]
    fn test_created_directory_is_modified() {
        // Same as above, but the path that appeared is a directory.
        let temp_dir = TempDir::with_prefix("test_created_directory").unwrap();
        let dir_path = temp_dir.path().join("appeared-dir");
        std::fs::create_dir(&dir_path).unwrap();
        let mtime =
            truncate_to_seconds(std::fs::metadata(&dir_path).unwrap().modified().unwrap()).unwrap();

        let missing_row = FileInputDesc {
            path: dir_path,
            is_directory: false,
            recursive: false,
            content_hash: None,
            modified_at: mtime,
        };

        assert!(matches!(
            check_file_state(&missing_row),
            Ok(FileState::Modified { .. })
        ));
    }

    #[test]
    fn test_still_missing_file_is_unchanged() {
        let temp_dir = TempDir::with_prefix("test_still_missing").unwrap();
        let missing_row = FileInputDesc {
            path: temp_dir.path().join("never-existed.txt"),
            is_directory: false,
            recursive: false,
            content_hash: None,
            modified_at: UNIX_EPOCH,
        };

        assert!(matches!(
            check_file_state(&missing_row),
            Ok(FileState::Unchanged)
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
            recursive: false,
            content_hash: content_hash.clone(),
            modified_at: UNIX_EPOCH,
        });
        let file2 = Input::File(FileInputDesc {
            path: path.clone(),
            is_directory: false,
            recursive: false,
            content_hash: content_hash.clone(),
            modified_at: UNIX_EPOCH + std::time::Duration::from_secs(1),
        });

        let mut inputs = vec![file1, file2.clone()];
        inputs.sort();
        inputs.dedup_by(Input::dedup);
        assert!(inputs.len() == 1);
        assert_eq!(inputs[0], file2);
    }

    /// Reproduces https://github.com/cachix/devenv/issues/2886
    ///
    /// When Nix copies a source tree into the store (`EvalOp::CopiedSource`, e.g.
    /// `languages.rust.import ./.`), the whole tree content is what ends up in the
    /// store. The eval cache must therefore detect changes to file *contents*
    /// nested inside the directory, not just the top-level entry names.
    ///
    /// Currently a directory's content hash is derived from the sorted list of its
    /// immediate child names only, so editing a nested file leaves the hash
    /// unchanged and `devenv build` returns a stale output path.
    #[test]
    fn test_directory_hash_changes_on_nested_content() {
        let temp_dir = TempDir::with_prefix("test_directory_hash_nested").unwrap();
        let src = temp_dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let main_rs = src.join("main.rs");
        std::fs::write(&main_rs, b"fn main() { println!(\"Hello, world!\"); }").unwrap();

        let before = FileInputDesc::new(temp_dir.path().to_path_buf(), UNIX_EPOCH, true).unwrap();
        assert!(before.is_directory);

        // Change a nested file's content without adding/removing any entries, so
        // the top-level directory listing stays identical.
        std::fs::write(&main_rs, b"fn main() { println!(\"Goodbye, world!\"); }").unwrap();

        let after = FileInputDesc::new(temp_dir.path().to_path_buf(), UNIX_EPOCH, true).unwrap();

        assert_ne!(
            before.content_hash, after.content_hash,
            "directory content hash must change when a nested file's content changes"
        );
    }

    /// `check_file_state` must report `Modified` when a file nested inside a
    /// recursively-tracked directory changes content, even though editing a
    /// nested file does not bump the top-level directory's mtime.
    #[test]
    fn test_check_state_detects_nested_content_change() {
        let temp_dir = TempDir::with_prefix("test_check_state_nested").unwrap();
        let src = temp_dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let main_rs = src.join("main.rs");
        std::fs::write(&main_rs, b"fn main() { println!(\"Hello, world!\"); }").unwrap();

        let desc = FileInputDesc::new(temp_dir.path().to_path_buf(), UNIX_EPOCH, true).unwrap();

        // Edit a nested file. The top-level directory's mtime is unaffected.
        std::fs::write(&main_rs, b"fn main() { println!(\"Goodbye, world!\"); }").unwrap();

        assert!(
            matches!(check_file_state(&desc), Ok(FileState::Modified { .. })),
            "nested content change must invalidate a recursively-tracked directory"
        );
    }

    #[test]
    fn test_check_state_detects_executable_bit_change_for_copied_file() {
        let temp_dir = TempDir::with_prefix("test_check_state_executable").unwrap();
        let file_path = temp_dir.path().join("script.sh");
        std::fs::write(&file_path, b"#!/bin/sh\nexit 0\n").unwrap();

        let mut permissions = std::fs::metadata(&file_path).unwrap().permissions();
        permissions.set_mode(0o644);
        std::fs::set_permissions(&file_path, permissions.clone()).unwrap();
        let desc = FileInputDesc::new(file_path.clone(), UNIX_EPOCH, true).unwrap();

        permissions.set_mode(0o744);
        std::fs::set_permissions(&file_path, permissions).unwrap();

        assert!(
            matches!(check_file_state(&desc), Ok(FileState::Modified { .. })),
            "changing the executable bit must invalidate a copied file"
        );
    }

    /// A non-recursive directory (e.g. tracked via `readDir`) only depends on its
    /// listing, so a nested content change must not invalidate it.
    #[test]
    fn test_check_state_ignores_nested_change_for_non_recursive_dir() {
        let temp_dir = TempDir::with_prefix("test_check_state_non_recursive").unwrap();
        let src = temp_dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let main_rs = src.join("main.rs");
        std::fs::write(&main_rs, b"fn main() { println!(\"Hello, world!\"); }").unwrap();

        let desc = FileInputDesc::new(temp_dir.path().to_path_buf(), UNIX_EPOCH, false).unwrap();

        std::fs::write(&main_rs, b"fn main() { println!(\"Goodbye, world!\"); }").unwrap();

        assert!(
            matches!(check_file_state(&desc), Ok(FileState::Unchanged)),
            "nested content change must not invalidate a non-recursive directory"
        );
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
