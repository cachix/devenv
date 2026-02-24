use fd_lock::RwLock;
use miette::{IntoDiagnostic, Result, miette};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Safely write a file with locking, avoiding writing if the content hasn't changed.
///
/// Returns Ok(true) if the file was written, Ok(false) if no write was needed.
pub fn write_file_with_lock<P: AsRef<Path>, S: AsRef<str>>(path: P, content: S) -> Result<bool> {
    let path = path.as_ref();
    let content = content.as_ref();

    // Create parent directories if they don't exist
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .into_diagnostic()
            .map_err(|e| miette!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    // Open or create the file with locking
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .into_diagnostic()
        .map_err(|e| miette!("Failed to open file {}: {}", path.display(), e))?;

    // Acquire an exclusive lock on the file
    let mut file_lock = RwLock::new(file);
    let mut file_handle = file_lock
        .write()
        .into_diagnostic()
        .map_err(|e| miette!("Failed to lock file {}: {}", path.display(), e))?;

    // Read existing content from the locked file handle.
    // IMPORTANT: We must read via file_handle (not fs::read_to_string) to avoid a race condition
    // where concurrent processes could read stale content through a separate file handle.
    let mut existing_content = String::new();
    file_handle
        .read_to_string(&mut existing_content)
        .into_diagnostic()
        .map_err(|e| miette!("Failed to read file {}: {}", path.display(), e))?;

    // Compare and write only if different
    if content != existing_content {
        // Seek to beginning before truncating and writing
        file_handle
            .seek(SeekFrom::Start(0))
            .into_diagnostic()
            .map_err(|e| miette!("Failed to seek in file {}: {}", path.display(), e))?;

        file_handle
            .set_len(0)
            .into_diagnostic()
            .map_err(|e| miette!("Failed to truncate file {}: {}", path.display(), e))?;

        file_handle
            .write_all(content.as_bytes())
            .into_diagnostic()
            .map_err(|e| miette!("Failed to write to file {}: {}", path.display(), e))?;

        // File was written
        Ok(true)
    } else {
        // No write needed
        Ok(false)
    }
}

pub fn expand_path(path: &str) -> String {
    let mut expanded = path.to_string();

    let re_braces = regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    let re_simple = regex::Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();

    expanded = re_braces
        .replace_all(&expanded, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_default()
        })
        .into_owned();

    expanded = re_simple
        .replace_all(&expanded, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_default()
        })
        .into_owned();

    expanded
}
