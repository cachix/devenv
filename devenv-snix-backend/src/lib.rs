//! Snix backend implementation for devenv.
//!
//! Stub implementation. The structure is fixed to match the
//! `Evaluator` + `Store` traits; bodies bail until the snix
//! integration is complete.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use devenv_core::bootstrap_args::BootstrapArgs;
use devenv_core::evaluator::{BuildOptions, Evaluator};
use devenv_core::store::{GcOptions, GcStats, PathInfo, Store, StorePath};
use devenv_core::{DevenvPaths, NixSettings, PortAllocator};
use miette::{Result, bail};

pub struct SnixStore;

#[async_trait(?Send)]
impl Store for SnixStore {
    fn uri(&self) -> &str {
        "snix"
    }

    async fn add_gc_root(&self, _gc_root: &Path, _store_path: &StorePath) -> Result<()> {
        bail!("SnixStore::add_gc_root is not yet implemented")
    }

    async fn realise(&self, _drv: &StorePath) -> Result<Vec<StorePath>> {
        bail!("SnixStore::realise is not yet implemented")
    }

    async fn is_trusted_user(&self) -> Result<bool> {
        bail!("SnixStore::is_trusted_user is not yet implemented")
    }

    async fn query_path_info(&self, _p: &StorePath) -> Result<Option<PathInfo>> {
        Ok(None)
    }

    async fn collect_garbage(&self, _opts: GcOptions) -> Result<GcStats> {
        bail!("SnixStore::collect_garbage is not yet implemented")
    }

    async fn copy_paths(&self, _dest: &dyn Store, _paths: &[StorePath]) -> Result<()> {
        bail!("SnixStore::copy_paths is not yet implemented")
    }
}

pub struct SnixBackend {
    #[allow(dead_code)]
    nix_settings: NixSettings,
    #[allow(dead_code)]
    paths: DevenvPaths,
    #[allow(dead_code)]
    bootstrap_args: Arc<BootstrapArgs>,
    store: SnixStore,
    #[allow(dead_code)]
    port_allocator: Arc<PortAllocator>,
}

impl SnixBackend {
    pub fn new(
        nix_settings: NixSettings,
        paths: DevenvPaths,
        bootstrap_args: Arc<BootstrapArgs>,
        port_allocator: Arc<PortAllocator>,
    ) -> Result<Self> {
        Ok(Self {
            nix_settings,
            paths,
            bootstrap_args,
            store: SnixStore,
            port_allocator,
        })
    }

    pub fn invalidate_eval_state(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait(?Send)]
impl Evaluator for SnixBackend {
    fn name(&self) -> &str {
        "snix"
    }

    fn store(&self) -> &dyn Store {
        &self.store
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn eval(&self, _attrs: &[&str]) -> Result<String> {
        bail!("SnixBackend::eval is not yet implemented")
    }

    async fn build(&self, _attrs: &[&str], _opts: BuildOptions) -> Result<Vec<StorePath>> {
        bail!("SnixBackend::build is not yet implemented")
    }
}
