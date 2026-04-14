use std::path::{Path, PathBuf};

use devenv_activity::{ActivityInstrument, activity};
use miette::Result;
use tokio::fs;

use super::Devenv;

impl Devenv {
    /// Garbage collect devenv environments and store paths.
    /// Returns (paths_deleted, bytes_freed).
    pub async fn gc(&self) -> Result<(u64, u64)> {
        let (to_gc, _removed_symlinks) = {
            let activity = activity!(
                INFO,
                operation,
                format!(
                    "Removing non-existing symlinks in {}",
                    &self.devenv_home_gc.display()
                )
            );
            cleanup_symlinks(&self.devenv_home_gc)
                .in_activity(&activity)
                .await
        };

        let (paths_deleted, bytes_freed) = {
            let activity = activity!(INFO, operation, "Running garbage collection");
            self.nix.gc(to_gc).in_activity(&activity).await?
        };

        Ok((paths_deleted, bytes_freed))
    }
}

async fn cleanup_symlinks(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    use futures::StreamExt;
    use tokio_stream::wrappers::ReadDirStream;

    if !root.exists() {
        fs::create_dir_all(root)
            .await
            .expect("Failed to create gc directory");
    }

    let read_dir = fs::read_dir(root).await.expect("Failed to read directory");

    let results: Vec<_> = ReadDirStream::new(read_dir)
        .filter_map(|e| async { e.ok() })
        .map(|e| e.path())
        .filter(|p| std::future::ready(p.is_symlink()))
        .map(|path| async move {
            if !path.exists() {
                // Dangling symlink - delete it
                if fs::remove_file(&path).await.is_ok() {
                    (None, Some(path))
                } else {
                    (None, None)
                }
            } else {
                match fs::canonicalize(&path).await {
                    Ok(target) => (Some(target), None),
                    Err(_) => (None, None),
                }
            }
        })
        .buffer_unordered(100)
        .collect()
        .await;

    let mut to_gc = Vec::new();
    let mut removed_symlinks = Vec::new();
    for (target, removed) in results {
        if let Some(t) = target {
            to_gc.push(t);
        }
        if let Some(r) = removed {
            removed_symlinks.push(r);
        }
    }

    (to_gc, removed_symlinks)
}
