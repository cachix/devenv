use fd_lock::RwLock;
use miette::{IntoDiagnostic, Result, miette};
use std::fs;
use std::io::Write;
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

    // Read existing content
    let existing_content = fs::read_to_string(path).unwrap_or_default();

    // Compare and write only if different
    if content != existing_content {
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
