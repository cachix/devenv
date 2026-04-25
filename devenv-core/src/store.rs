//! Generic Nix store interface.
//!
//! Any "thing that talks to a Nix store" — local daemon, remote daemon,
//! `rust-plugin://` (snix-backed), or a future Rust-native `snix-store` —
//! implements this trait. Operations are limited to what consumers above
//! the evaluator (cachix push, GC roots, copy-paths) need.
//!
//! No evaluator concepts here. See [`crate::evaluator::Evaluator`].

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use miette::Result;

/// A store path, by string. Backends parse on demand.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorePath(pub PathBuf);

impl StorePath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}

impl From<PathBuf> for StorePath {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

impl From<&Path> for StorePath {
    fn from(p: &Path) -> Self {
        Self(p.to_path_buf())
    }
}

impl AsRef<Path> for StorePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for StorePath {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// Garbage-collection options.
#[derive(Clone, Debug, Default)]
pub struct GcOptions {
    /// Delete only specific paths (intersection of GC closure with this list).
    pub paths: Option<Vec<StorePath>>,
    /// Maximum bytes to free before stopping (0 = unbounded).
    pub max_freed: u64,
}

/// Stats returned by [`Store::collect_garbage`].
#[derive(Clone, Debug, Default)]
pub struct GcStats {
    pub paths_deleted: u64,
    pub bytes_freed: u64,
}

/// Subset of `nix path-info` for callers that need it (cachix lookups, etc.).
#[derive(Clone, Debug)]
pub struct PathInfo {
    pub path: StorePath,
    pub nar_size: u64,
    pub references: Vec<StorePath>,
}

/// Generic Nix store interface.
#[async_trait(?Send)]
pub trait Store: Send + Sync {
    /// Store URI (e.g. `auto`, `daemon`, `local?root=…`, `rust-plugin://memory`).
    fn uri(&self) -> &str;

    /// Pin `store_path` against GC by making `gc_root` symlink to it.
    async fn add_gc_root(&self, gc_root: &Path, store_path: &StorePath) -> Result<()>;

    /// Build (or substitute) `drv` and return its output paths.
    async fn realise(&self, drv: &StorePath) -> Result<Vec<StorePath>>;

    /// Whether the current user is trusted by the daemon.
    async fn is_trusted_user(&self) -> Result<bool>;

    /// Return path info, or `None` if the path is unknown to the store.
    async fn query_path_info(&self, p: &StorePath) -> Result<Option<PathInfo>>;

    /// Run garbage collection.
    async fn collect_garbage(&self, opts: GcOptions) -> Result<GcStats>;

    /// Copy paths from `self` to `dest`.
    async fn copy_paths(&self, dest: &dyn Store, paths: &[StorePath]) -> Result<()>;
}
