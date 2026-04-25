//! Backend dispatcher.
//!
//! Lives in the orchestration crate (not `devenv-core`) because it imports
//! the concrete backend crates, which sit above `devenv-core` in the
//! dependency graph.

use std::path::PathBuf;
use std::sync::Arc;

use devenv_core::{
    BootstrapArgs, CacheSettings, CachixManager, DevenvPaths, NixBackend, NixSettings,
    PortAllocator,
    config::{NixBackendType, NixpkgsConfig},
};
use miette::{Result, WrapErr};
use sqlx::SqlitePool;
use tokio::sync::OnceCell;
use tokio_shutdown::Shutdown;

/// Construct a fully-initialized backend, picking the implementation based
/// on `backend_type`.
#[allow(clippy::too_many_arguments)]
pub async fn init_backend(
    backend_type: NixBackendType,
    bootstrap_args: BootstrapArgs,
    paths: DevenvPaths,
    nixpkgs_config: NixpkgsConfig,
    nix_settings: NixSettings,
    cache_settings: CacheSettings,
    cachix_manager: Arc<CachixManager>,
    shutdown: Arc<Shutdown>,
    eval_cache_pool: Option<Arc<OnceCell<SqlitePool>>>,
    port_allocator: Arc<PortAllocator>,
    store: Option<PathBuf>,
) -> Result<Box<dyn NixBackend>> {
    match backend_type {
        NixBackendType::Nix => {
            let backend = devenv_nix_backend::nix_backend::NixCBackend::init(
                bootstrap_args,
                paths,
                nixpkgs_config,
                nix_settings,
                cache_settings,
                cachix_manager,
                shutdown,
                eval_cache_pool,
                store,
                port_allocator,
            )
            .await
            .wrap_err("Failed to initialize Nix backend")?;
            Ok(Box::new(backend))
        }
        #[cfg(feature = "snix")]
        NixBackendType::Snix => {
            let backend = devenv_snix_backend::SnixBackend::init(
                bootstrap_args,
                nix_settings,
                cache_settings,
                paths,
                cachix_manager,
                eval_cache_pool,
            )
            .await
            .wrap_err("Failed to initialize Snix backend")?;
            Ok(Box::new(backend))
        }
    }
}
