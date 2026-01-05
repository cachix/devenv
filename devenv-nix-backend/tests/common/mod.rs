//! Common test utilities shared across test files

pub mod mock_cachix_daemon;

use devenv_core::cachix::{CachixManager, CachixPaths};
use std::path::Path;
use std::sync::Arc;

/// Macro for async tests that properly registers tokio worker threads with Boehm GC.
///
/// This is needed because Nix uses Boehm GC with parallel marking,
/// and GC must know about all threads that access GC-managed memory.
/// The standard `#[tokio::test]` doesn't support `on_thread_start`, so we
/// need this custom macro to match production runtime behavior.
///
/// Usage:
/// ```ignore
/// gc_test!(async fn test_name() {
///     // async test body
/// });
///
/// gc_test!(#[ignore] async fn ignored_test() {
///     // ignored test body
/// });
/// ```
#[macro_export]
macro_rules! gc_test {
    (#[ignore] async fn $name:ident() $body:block) => {
        #[test]
        #[ignore]
        fn $name() {
            // Initialize Nix/GC BEFORE spawning worker threads
            devenv_nix_backend::nix_init();

            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .on_thread_start(|| {
                    use devenv_nix_backend::gc_register_current_thread;
                    let _ = gc_register_current_thread();
                })
                .build()
                .expect("Failed to create test runtime")
                .block_on(async $body)
        }
    };
    (async fn $name:ident() $body:block) => {
        #[test]
        fn $name() {
            // Initialize Nix/GC BEFORE spawning worker threads
            devenv_nix_backend::nix_init();

            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .on_thread_start(|| {
                    use devenv_nix_backend::gc_register_current_thread;
                    let _ = gc_register_current_thread();
                })
                .build()
                .expect("Failed to create test runtime")
                .block_on(async $body)
        }
    };
}

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
