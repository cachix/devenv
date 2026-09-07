//! `Store` impl backed by the C-Nix FFI store handle.

use std::path::Path;

use async_trait::async_trait;
use devenv_core::store::{GcOptions, GcStats, PathInfo, Store as StoreTrait, StorePath};
use miette::{Result, WrapErr, miette};
use nix_bindings_store::store::Store;

use crate::anyhow_ext::AnyhowToMiette;
use crate::gc_root::ensure_gc_root;
use crate::gc_store;

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
    fn uri(&self) -> Result<String> {
        self.inner
            .clone()
            .get_uri()
            .to_miette()
            .wrap_err("Failed to query Nix store URI")
    }

    async fn add_gc_root(&self, gc_root: &Path, store_path: &StorePath) -> Result<()> {
        let mut store = self.inner.clone();
        ensure_gc_root(&mut store, gc_root, store_path.as_str()).map(|_| ())
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
        gc_store::collect_garbage(&mut store, opts).await
    }

    async fn copy_paths(&self, _dest: &dyn StoreTrait, _paths: &[StorePath]) -> Result<()> {
        Err(miette!(
            "CNixStore::copy_paths: not yet implemented across Store trait"
        ))
    }
}
