use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use devenv_activity::{Activity, ActivityInstrument, activity};
use miette::Result;
use tokio::fs;

use super::Devenv;

impl Devenv {
    /// Garbage collect devenv environments and store paths.
    /// Returns (paths_deleted, bytes_freed).
    pub async fn gc(&self) -> Result<(u64, u64)> {
        let (to_gc, _removed_symlinks) = {
            let activity = activity!(INFO, operation, "Scanning environment history");
            cleanup_symlinks(&self.devenv_home_gc, &activity)
                .in_activity(&activity)
                .await
        };

        let stats = {
            let activity = activity!(INFO, operation, "Running garbage collection");
            self.backend().gc(to_gc).in_activity(&activity).await?
        };
        let paths_deleted = stats.paths_deleted;
        let bytes_freed = stats.bytes_freed;

        Ok((paths_deleted, bytes_freed))
    }
}

async fn cleanup_symlinks(root: &Path, activity: &Activity) -> (Vec<PathBuf>, Vec<PathBuf>) {
    use futures::StreamExt;
    use tokio_stream::wrappers::ReadDirStream;

    if !root.exists() {
        fs::create_dir_all(root)
            .await
            .expect("Failed to create gc directory");
    }

    let read_dir = fs::read_dir(root).await.expect("Failed to read directory");

    let paths: Vec<_> = ReadDirStream::new(read_dir)
        .filter_map(|e| async { e.ok() })
        .map(|e| e.path())
        .filter(|p| std::future::ready(p.is_symlink()))
        .collect()
        .await;
    let total = paths.len() as u64;
    let completed = AtomicU64::new(0);
    activity.progress(0, total, Some(&format!("checking {}", root.display())));

    let results: Vec<_> = futures::stream::iter(paths)
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
        .inspect(|_| {
            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            activity.progress(done, total, None);
        })
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

    activity.progress(
        total,
        total,
        Some(&format!(
            "{} environments; removed {} stale links",
            to_gc.len(),
            removed_symlinks.len()
        )),
    );

    (to_gc, removed_symlinks)
}
