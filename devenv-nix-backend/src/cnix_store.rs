//! `Store` impl backed by the C-Nix FFI store handle.

use std::path::Path;

use async_trait::async_trait;
use devenv_activity::activity;
use devenv_core::store::{GcOptions, GcStats, PathInfo, Store as StoreTrait, StorePath};
use miette::{Result, WrapErr, miette};
use nix_bindings_store::store::{GcAction, Store};

use crate::anyhow_ext::AnyhowToMiette;

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

#[async_trait(?Send)]
impl StoreTrait for CNixStore {
    fn uri(&self) -> &str {
        "cnix"
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
        let mut stats = GcStats::default();
        match opts.paths {
            Some(paths) => {
                let total = paths.len() as u64;
                let activity = activity!(INFO, operation, "Deleting store paths");
                for (i, path) in paths.iter().enumerate() {
                    let path_str = path.as_str();
                    let path_name = path
                        .as_path()
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(path_str);
                    activity.progress(i as u64, total, Some(path_name));

                    let parsed = match store.parse_store_path(path_str).to_miette() {
                        Ok(sp) => sp,
                        Err(_) => {
                            // Not a valid store path — treat it as a plain
                            // file or directory to clean up. Mirrors the
                            // pre-refactor behavior in NixCBackend::gc.
                            let p = path.as_path();
                            let _ = std::fs::remove_file(p).or_else(|_| std::fs::remove_dir_all(p));
                            continue;
                        }
                    };
                    if let Ok((deleted, bytes)) = store.collect_garbage(
                        GcAction::DeleteSpecific,
                        Some(&[&parsed]),
                        false,
                        opts.max_freed,
                    ) {
                        stats.paths_deleted += deleted.len() as u64;
                        stats.bytes_freed += bytes;
                    }
                }
                activity.progress(total, total, None);
            }
            None => {
                let (deleted, bytes) = store
                    .collect_garbage(GcAction::DeleteDead, None, false, opts.max_freed)
                    .to_miette()
                    .wrap_err("Failed to run garbage collection")?;
                stats.paths_deleted = deleted.len() as u64;
                stats.bytes_freed = bytes;
            }
        }
        Ok(stats)
    }

    async fn copy_paths(&self, _dest: &dyn StoreTrait, _paths: &[StorePath]) -> Result<()> {
        Err(miette!(
            "CNixStore::copy_paths: not yet implemented across Store trait"
        ))
    }
}
