//! `Store` impl backed by the C-Nix FFI store handle.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use devenv_activity::{Activity, activity};
use devenv_core::store::{GcOptions, GcStats, PathInfo, Store as StoreTrait, StorePath};
use miette::{Result, WrapErr, miette};
use nix_bindings_store::store::{GcAction, Store};

use crate::anyhow_ext::AnyhowToMiette;

const BATCHED_GC_MIN_VERSION: (u64, u64) = (2, 35);

pub struct CNixStore {
    inner: Store,
}

impl CNixStore {
    pub fn new(inner: Store) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Store {
        &self.inner
    }
}

// SAFETY: the underlying store handle is internally synchronized at the
// C-Nix daemon boundary. Mirrors the contract on `NixCBackend`.
unsafe impl Send for CNixStore {}
unsafe impl Sync for CNixStore {}

/// Delete paths that aren't store paths (plain files or directories).
///
/// Offloaded because `remove_dir_all` recurses over the whole tree, which
/// can stall a runtime thread for arbitrarily long on a large GC root.
pub(crate) async fn remove_plain_paths(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || {
        for path in paths {
            let _ = std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir_all(&path));
        }
    })
    .await;
}

fn supports_batched_gc(version: &str) -> bool {
    let mut components = version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|component| !component.is_empty())
        .filter_map(|component| component.parse::<u64>().ok());
    let Some(major) = components.next() else {
        return false;
    };
    let Some(minor) = components.next() else {
        return false;
    };
    (major, minor) >= BATCHED_GC_MIN_VERSION
}

fn legacy_collect_garbage(
    store: &mut Store,
    mut remaining: Vec<nix_bindings_store::path::StorePath>,
    max_freed: u64,
    activity: &Activity,
) -> GcStats {
    let total = remaining.len() as u64;
    let mut stats = GcStats::default();
    let mut processed = 0u64;
    let mut pass = 0u64;

    activity.progress(0, total, Some(&format!("collecting {total} paths")));

    while !remaining.is_empty() {
        pass += 1;
        let mut retry = Vec::new();
        let mut deleted_this_pass = false;

        for path in remaining {
            if max_freed != 0 && stats.bytes_freed >= max_freed {
                retry.push(path);
                continue;
            }

            let name = path.name().unwrap_or_else(|_| "store path".to_string());
            activity.progress(processed, total, Some(&name));
            let call_max_freed = if max_freed == 0 {
                0
            } else {
                max_freed.saturating_sub(stats.bytes_freed)
            };
            match store.collect_garbage(
                GcAction::DeleteSpecific,
                Some(&[&path]),
                false,
                false,
                call_max_freed,
            ) {
                Ok((deleted, bytes)) => {
                    deleted_this_pass |= !deleted.is_empty();
                    stats.paths_deleted += deleted.len() as u64;
                    stats.bytes_freed += bytes;
                }
                Err(error) => {
                    tracing::debug!(%error, path = %name, "store path is still live or could not be deleted");
                    retry.push(path);
                }
            }
            if pass == 1 {
                processed += 1;
            }
        }

        if !deleted_this_pass || (max_freed != 0 && stats.bytes_freed >= max_freed) {
            remaining = retry;
            break;
        }
        remaining = retry;
    }

    let retained = remaining.len();
    activity.progress(
        total,
        total,
        Some(&format!(
            "deleted {} paths; retained or skipped {retained} paths",
            stats.paths_deleted
        )),
    );
    stats
}

#[async_trait(?Send)]
impl StoreTrait for CNixStore {
    fn uri(&self) -> Result<String> {
        self.inner
            .clone()
            .get_uri()
            .to_miette()
            .wrap_err("Failed to query Nix store URI")
    }

    async fn add_gc_root(&self, gc_root: &Path, store_path: &StorePath) -> Result<()> {
        let mut store = self.inner.clone();
        let parsed = store
            .parse_store_path(store_path.as_str())
            .to_miette()
            .wrap_err("Failed to parse store path")?;
        if gc_root.symlink_metadata().is_ok() {
            std::fs::remove_file(gc_root)
                .map_err(|e| miette!("Failed to remove existing GC root: {}", e))?;
        }
        store
            .add_perm_root(&parsed, gc_root)
            .to_miette()
            .wrap_err("Failed to create GC root")
    }

    async fn realise(&self, drv: &StorePath) -> Result<Vec<StorePath>> {
        let _ = drv;
        Err(miette!(
            "CNixStore::realise: route through NixCBackend::build for now"
        ))
    }

    async fn is_trusted_user(&self) -> Result<bool> {
        let mut store = self.inner.clone();
        match store.is_trusted_client() {
            nix_bindings_store::store::TrustedFlag::Trusted => Ok(true),
            nix_bindings_store::store::TrustedFlag::NotTrusted => Ok(false),
            nix_bindings_store::store::TrustedFlag::Unknown => {
                Err(miette!("Unable to determine trust status for Nix store"))
            }
        }
    }

    async fn query_path_info(&self, _p: &StorePath) -> Result<Option<PathInfo>> {
        Ok(None)
    }

    async fn collect_garbage(&self, opts: GcOptions) -> Result<GcStats> {
        let mut store = self.inner.clone();
        match opts.paths {
            Some(paths) => {
                let total = paths.len() as u64;
                let (plain_paths, store_paths) =
                    activity!(INFO, operation, "Preparing garbage collection").scoped(|prepare| {
                        let mut plain_paths: Vec<PathBuf> = Vec::new();
                        let mut seen_paths = HashSet::new();
                        let mut store_paths = Vec::new();
                        for (i, path) in paths.iter().enumerate() {
                            let path_str = path.as_str();
                            let path_name = path
                                .as_path()
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path_str);
                            prepare.progress(i as u64, total, Some(path_name));

                            if !seen_paths.insert(path.as_path().to_path_buf()) {
                                continue;
                            }

                            match store.parse_store_path(path_str).to_miette() {
                                Ok(store_path) => store_paths.push(store_path),
                                Err(_) => {
                                    // Not a valid store path — treat it as a plain
                                    // file or directory to clean up. Mirrors the
                                    // pre-refactor behavior in NixCBackend::gc.
                                    plain_paths.push(path.as_path().to_path_buf());
                                }
                            }
                        }
                        prepare.progress(
                            total,
                            total,
                            Some(&format!(
                                "{} unique store paths; {} stale paths",
                                store_paths.len(),
                                plain_paths.len()
                            )),
                        );
                        (plain_paths, store_paths)
                    });

                let mut stats = GcStats::default();
                if !store_paths.is_empty() {
                    let store_path_refs: Vec<_> = store_paths.iter().collect();
                    let closure = activity!(INFO, operation, "Finding unused store paths").scoped(
                        |closure_activity| -> Result<_> {
                            closure_activity.progress(
                                0,
                                1,
                                Some(&format!(
                                    "computing the closure of {} environments",
                                    store_paths.len()
                                )),
                            );
                            let closure = store
                                .compute_fs_closure(&store_path_refs, false, false, false)
                                .inspect_err(|_| closure_activity.fail())
                                .to_miette()
                                .wrap_err("Failed to compute environment closure")?;
                            closure_activity.progress(
                                1,
                                1,
                                Some(&format!("{} candidate store paths", closure.len())),
                            );
                            Ok(closure)
                        },
                    )?;

                    let daemon_version = store.get_version().unwrap_or_default();
                    let batched = supports_batched_gc(&daemon_version);
                    tracing::debug!(
                        nix_daemon_version = %daemon_version,
                        batched_gc = batched,
                        "selected garbage collection mode"
                    );
                    activity!(INFO, operation, "Deleting unused store paths").scoped(
                        |delete_activity| -> Result<()> {
                            if batched {
                                delete_activity.progress(
                                    0,
                                    1,
                                    Some(&format!("collecting {} paths", closure.len())),
                                );
                                let closure_refs: Vec<_> = closure.iter().collect();
                                let (deleted, bytes) = store
                                    .collect_garbage(
                                        GcAction::DeleteDead,
                                        Some(&closure_refs),
                                        false,
                                        true,
                                        opts.max_freed,
                                    )
                                    .inspect_err(|_| delete_activity.fail())
                                    .to_miette()
                                    .wrap_err("Failed to delete unused store paths")?;
                                stats.paths_deleted = deleted.len() as u64;
                                stats.bytes_freed = bytes;
                                delete_activity.progress(
                                    1,
                                    1,
                                    Some(&format!("deleted {} store paths", deleted.len())),
                                );
                            } else {
                                stats = legacy_collect_garbage(
                                    &mut store,
                                    closure,
                                    opts.max_freed,
                                    delete_activity,
                                );
                            }
                            Ok(())
                        },
                    )?;
                }

                if !plain_paths.is_empty() {
                    let cleanup_activity = activity!(INFO, operation, "Removing stale paths");
                    cleanup_activity.progress(
                        0,
                        1,
                        Some(&format!("{} non-store paths", plain_paths.len())),
                    );
                    remove_plain_paths(plain_paths).await;
                    cleanup_activity.progress(1, 1, None);
                }
                Ok(stats)
            }
            None => {
                let activity = activity!(INFO, operation, "Deleting unused store paths");
                activity.progress(0, 1, Some("scanning the Nix store"));
                let (deleted, bytes) = activity
                    .in_scope(|| {
                        store.collect_garbage(
                            GcAction::DeleteDead,
                            None,
                            false,
                            false,
                            opts.max_freed,
                        )
                    })
                    .inspect_err(|_| activity.fail())
                    .to_miette()
                    .wrap_err("Failed to run garbage collection")?;
                activity.progress(
                    1,
                    1,
                    Some(&format!("deleted {} store paths", deleted.len())),
                );
                Ok(GcStats {
                    paths_deleted: deleted.len() as u64,
                    bytes_freed: bytes,
                })
            }
        }
    }

    async fn copy_paths(&self, _dest: &dyn StoreTrait, _paths: &[StorePath]) -> Result<()> {
        Err(miette!(
            "CNixStore::copy_paths: not yet implemented across Store trait"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::supports_batched_gc;

    #[test]
    fn detects_batched_gc_daemon_versions() {
        assert!(supports_batched_gc("2.35.0"));
        assert!(supports_batched_gc("2.35.0pre20260801"));
        assert!(supports_batched_gc("Nix 2.36.1"));
        assert!(!supports_batched_gc("2.34.4"));
        assert!(!supports_batched_gc(""));
        assert!(!supports_batched_gc("unknown"));
    }
}
