//! Common test utilities shared across test files

use devenv_core::cachix::{CachixManager, CachixPaths};
use devenv_core::{
    CacheOptions, CacheSettings, Config, DevenvPaths, NixOptions, NixSettings, PortAllocator,
};
use devenv_nix_backend::nix_backend::NixRustBackend;
use std::path::Path;
use std::sync::Arc;
use tokio_shutdown::Shutdown;

/// Get the current Nix system string based on architecture and OS
pub fn get_current_system() -> &'static str {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    match (arch, os) {
        ("x86_64", "linux") => "x86_64-linux",
        ("aarch64", "linux") => "aarch64-linux",
        ("x86_64", "macos") => "x86_64-darwin",
        ("aarch64", "macos") => "aarch64-darwin",
        _ => panic!("Unsupported system: {arch}-{os}"),
    }
}

/// Create a test CachixManager with temporary paths
pub fn create_test_cachix_manager(
    base_dir: &Path,
    daemon_socket: Option<std::path::PathBuf>,
) -> Arc<CachixManager> {
    let cachix_paths = CachixPaths {
        trusted_keys: base_dir.join(".devenv/cachix/trusted-keys.json"),
        netrc: base_dir.join(".devenv/netrc"),
        daemon_socket,
    };
    Arc::new(CachixManager::new(cachix_paths))
}

/// Create a `NixRustBackend` from `NixOptions`, resolving settings internally.
pub fn create_backend(
    paths: DevenvPaths,
    config: Config,
    nix_cli: NixOptions,
    cachix_manager: Arc<CachixManager>,
    shutdown: Arc<Shutdown>,
) -> miette::Result<NixRustBackend> {
    let nix_settings = NixSettings::resolve(nix_cli, &config);
    let cache_settings = CacheSettings::resolve(CacheOptions::default());
    let nixpkgs_config = config.nixpkgs_config(&nix_settings.system);
    NixRustBackend::new(
        paths,
        nixpkgs_config,
        nix_settings,
        cache_settings,
        cachix_manager,
        shutdown,
        None,
        None,
        Arc::new(PortAllocator::new()),
    )
}
