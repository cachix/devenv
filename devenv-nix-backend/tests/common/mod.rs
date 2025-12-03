//! Common test utilities shared across test files

pub mod mock_cachix_daemon;

use devenv_core::cachix::{CachixManager, CachixPaths};
use std::path::Path;
use std::sync::Arc;

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
