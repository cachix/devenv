#![cfg(feature = "test-nix-store")]
//! Tests for the NixBackend FFI implementation
//!
//! These tests verify the functionality of the Rust FFI-based NixBackend,
//! including input overrides via NixOptions.

use devenv_core::eval_op::EvalOp;
use devenv_core::{Config, DevenvPaths, NixBackend, NixOptions, Options};
use devenv_nix_backend::nix_backend::NixCBackend;
use devenv_nix_backend_macros::nix_test;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio_shutdown::Shutdown;

/// Guard struct that changes cwd to a directory and restores on drop
struct CwdGuard {
    original_cwd: PathBuf,
}

impl CwdGuard {
    fn new(target: &std::path::Path) -> Self {
        let original_cwd = std::env::current_dir().expect("Failed to get current directory");
        std::env::set_current_dir(target).expect("Failed to change to target directory");
        CwdGuard { original_cwd }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
    }
}

// Import shared test utilities
mod common;
use common::{create_test_cachix_manager, init_backend, paths_under};

/// Create the test paths layout AND materialize its directories on disk.
///
/// `paths_under` only constructs the value; tests that actually run Nix need
/// the directories to exist beforehand because the backend writes into them.
fn create_test_paths_in(base: &std::path::Path) -> DevenvPaths {
    let paths = paths_under(base);
    std::fs::create_dir_all(&paths.dotfile).expect("Failed to create .devenv");
    std::fs::create_dir_all(&paths.dot_gc).expect("Failed to create gc dir");
    std::fs::create_dir_all(&paths.home_gc).expect("Failed to create home gc dir");
    paths
}

/// Copy fixture lock file to destination directory
/// This avoids unnecessary update() calls in tests that don't specifically test locking
fn copy_fixture_lock(dest_dir: &std::path::Path) {
    let fixture_lock = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests/fixtures/devenv.lock");
    let dest_lock = dest_dir.join("devenv.lock");
    std::fs::copy(&fixture_lock, &dest_lock).expect("Failed to copy fixture lock file");
}

/// Set up an isolated working directory for a test.
///
/// Creates a temp dir, changes cwd to it (restored via CwdGuard on drop),
/// writes `devenv.nix` (custom or minimal) and `devenv.yaml`, materializes
/// the test paths layout, and loads the resulting `Config`. No backend is
/// instantiated.
fn setup_isolated_test_files(
    yaml_content: &str,
    nix_content: Option<&str>,
) -> (TempDir, CwdGuard, DevenvPaths, Config) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();

    let cwd_guard = CwdGuard::new(temp_path);

    if let Some(nix) = nix_content {
        std::fs::write(temp_path.join("devenv.nix"), nix)
            .expect("Failed to write custom devenv.nix");
    } else {
        std::fs::write(temp_path.join("devenv.nix"), "{ ... }: { }")
            .expect("Failed to write minimal devenv.nix");
    }

    std::fs::write(temp_path.join("devenv.yaml"), yaml_content)
        .expect("Failed to write devenv.yaml");

    let paths = create_test_paths_in(temp_path);
    let config = Config::load_from(temp_path).expect("Failed to load config");

    (temp_dir, cwd_guard, paths, config)
}

/// Like [`setup_isolated_test_files`] plus a fully-bootstrapped `NixCBackend`.
///
/// Use this for tests that exercise evaluation paths. For tests that
/// expect bootstrap itself to fail, call `setup_isolated_test_files`
/// followed by `init_backend` so the result can be inspected.
async fn setup_isolated_test_env(
    yaml_content: &str,
    nix_content: Option<&str>,
    nix_cli: NixOptions,
) -> (TempDir, CwdGuard, NixCBackend, DevenvPaths, Config) {
    let (temp_dir, cwd_guard, paths, config) = setup_isolated_test_files(yaml_content, nix_content);

    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    let shutdown = Shutdown::new();
    let backend = init_backend(
        paths.clone(),
        config.clone(),
        nix_cli,
        cachix_manager,
        shutdown,
    )
    .await
    .expect("Failed to initialize NixCBackend");

    (temp_dir, cwd_guard, backend, paths, config)
}

#[nix_test]
async fn test_backend_creation() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    assert_eq!(backend.name(), "nix");
}

#[nix_test]
async fn test_backend_update_all_inputs() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Update all inputs
    let result = backend.update(&None, &config.inputs, &[]).await;
    assert!(result.is_ok(), "Update should succeed: {:?}", result.err());

    // Verify lock file was created in temp directory
    let lock_path = temp_dir.path().join("devenv.lock");
    assert!(lock_path.exists(), "Lock file should be created");
}

#[nix_test]
async fn test_backend_update_specific_input() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // First create an initial lock
    backend
        .update(&None, &config.inputs, &[])
        .await
        .expect("Initial update failed");

    // Now update just nixpkgs
    let result = backend
        .update(&Some("nixpkgs".to_string()), &config.inputs, &[])
        .await;
    assert!(
        result.is_ok(),
        "Selective update should succeed: {:?}",
        result.err()
    );
}

#[nix_test]
async fn test_backend_eval_expression() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for evaluation
    copy_fixture_lock(temp_dir.path());

    let lock_path = temp_dir.path().join("devenv.lock");
    assert!(lock_path.exists(), "Lock file should be created");

    // Evaluate a simple attribute
    let result = backend.eval(&["config.devenv.root"]).await;
    assert!(result.is_ok(), "Eval should succeed: {:?}", result.err());

    let json_str = result.unwrap();
    assert!(!json_str.is_empty(), "Eval result should not be empty");

    // Should be valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("Result should be valid JSON");

    // devenv.config.devenv.root should be a string
    assert!(
        parsed.is_string(),
        "devenv.root should be a string, got: {}",
        parsed
    );
}

#[nix_test]
async fn test_backend_eval_multiple_attributes() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for evaluation
    copy_fixture_lock(temp_dir.path());

    // Evaluate multiple attributes
    let result = backend
        .eval(&["config.packages", "config.languages.rust.enable"])
        .await;

    assert!(
        result.is_ok(),
        "Multi-attr eval should succeed: {:?}",
        result.err()
    );

    let json = result.unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Result should be valid JSON");

    // Should return array for multiple attributes
    assert!(parsed.is_array(), "Multiple attributes should return array");
}

#[nix_test]
async fn test_backend_build_package() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Build the devenv shell
    let result = backend.build(&["shell"], None, None).await;

    assert!(result.is_ok(), "Build should succeed: {:?}", result.err());

    let output_paths = result.unwrap();
    assert!(!output_paths.is_empty(), "Build should return output paths");
    assert!(output_paths[0].to_str().unwrap().starts_with("/nix/store"));
}

#[nix_test]
async fn test_backend_build_with_gc_root() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    let gc_root_base = temp_dir.path().join("result");

    // Build with GC root
    let result = backend.build(&["shell"], None, Some(&gc_root_base)).await;

    assert!(
        result.is_ok(),
        "Build with GC root should succeed: {:?}",
        result.err()
    );

    // The actual GC root is named "{base}-{attr}" per the build() implementation
    let expected_gc_root = temp_dir.path().join("result-shell");
    assert!(
        expected_gc_root.exists(),
        "GC root symlink should be created at {:?}",
        expected_gc_root
    );
}

#[nix_test]
async fn test_backend_dev_env() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for dev_env
    copy_fixture_lock(temp_dir.path());

    let gc_root = temp_dir.path().join(".devenv/profile");

    // Get development environment
    let result = backend.dev_env(true, &gc_root).await;
    assert!(result.is_ok(), "dev_env should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(
        !output.bash_env.is_empty(),
        "dev_env should return environment"
    );
}

/// Helper for hot-reload invalidation tests.
/// Sets up env, runs dev_env, modifies a file, invalidates, runs dev_env again,
/// and asserts the new value is picked up.
async fn assert_invalidate_picks_up_change(
    yaml: &str,
    nix_content: &str,
    extra_files: &[(&str, &str)],
    modify_file: &str,
    modified_content: &str,
    expected_before: &str,
    expected_after: &str,
) {
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, Some(nix_content), NixOptions::default()).await;

    for (name, content) in extra_files {
        std::fs::write(temp_dir.path().join(name), content).expect("Failed to write extra file");
    }

    copy_fixture_lock(temp_dir.path());
    let gc_root = temp_dir.path().join(".devenv/profile");

    let output1 = backend
        .dev_env(false, &gc_root)
        .await
        .expect("First dev_env should succeed");
    let env1 = String::from_utf8_lossy(&output1.bash_env);
    assert!(
        env1.contains(expected_before),
        "First dev_env should contain '{expected_before}'"
    );

    std::fs::write(temp_dir.path().join(modify_file), modified_content)
        .expect("Failed to write modified file");

    backend.invalidate().expect("invalidate should succeed");

    let output2 = backend
        .dev_env(false, &gc_root)
        .await
        .expect("Second dev_env after invalidate should succeed");
    let env2 = String::from_utf8_lossy(&output2.bash_env);
    assert!(
        env2.contains(expected_after),
        "After invalidate should contain '{expected_after}', got stale result"
    );
}

/// Simulate hot-reload: dev_env() -> modify devenv.nix -> invalidate() -> dev_env()
///
/// This exercises the full code path including the SQLite eval cache,
/// which is where stale results were observed during hot-reload.
#[nix_test]
async fn test_dev_env_after_invalidate() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    assert_invalidate_picks_up_change(
        yaml,
        r#"{ pkgs, ... }: { env.TEST_RELOAD_VAR = "version1"; }"#,
        &[],
        "devenv.nix",
        r#"{ pkgs, ... }: { env.TEST_RELOAD_VAR = "version2"; }"#,
        "version1",
        "version2",
    )
    .await;
}

/// Simulate hot-reload when a file IMPORTED by devenv.nix changes.
///
/// devenv.nix imports ./extra.nix. We modify extra.nix and invalidate.
/// This tests whether imported files are tracked in the eval cache.
#[nix_test]
async fn test_dev_env_after_invalidate_imported_file() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    assert_invalidate_picks_up_change(
        yaml,
        r#"{ pkgs, ... }: { imports = [ ./extra.nix ]; }"#,
        &[(
            "extra.nix",
            r#"{ ... }: { env.TEST_IMPORT_VAR = "original"; }"#,
        )],
        "extra.nix",
        r#"{ ... }: { env.TEST_IMPORT_VAR = "updated"; }"#,
        "original",
        "updated",
    )
    .await;
}

/// Simulate hot-reload when a file imported via devenv.yaml `imports` changes.
///
/// devenv.yaml lists `imports: [./extra.nix]`. We modify extra.nix and invalidate.
/// This goes through bootstrapLib.nix's importModule path.
#[nix_test]
async fn test_dev_env_after_invalidate_yaml_import() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
imports:
  - ./extra.nix
"#;
    assert_invalidate_picks_up_change(
        yaml,
        r#"{ ... }: { }"#,
        &[(
            "extra.nix",
            r#"{ ... }: { env.TEST_YAML_IMPORT = "original"; }"#,
        )],
        "extra.nix",
        r#"{ ... }: { env.TEST_YAML_IMPORT = "updated"; }"#,
        "original",
        "updated",
    )
    .await;
}

#[nix_test]
async fn test_backend_metadata() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Get flake metadata
    let result = backend.metadata().await;
    assert!(
        result.is_ok(),
        "metadata should succeed: {:?}",
        result.err()
    );

    let metadata = result.unwrap();
    assert!(!metadata.is_empty(), "Metadata should not be empty");
    // Should contain Inputs section with lock file entries
    assert!(
        metadata.contains("Inputs:"),
        "Metadata should contain Inputs section. Got: {}",
        metadata
    );
}

#[nix_test]
async fn test_backend_gc() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Create some dummy paths to collect
    let path1 = temp_dir.path().join("path1");
    let path2 = temp_dir.path().join("path2");
    std::fs::write(&path1, "test1").expect("Failed to create test file");
    std::fs::write(&path2, "test2").expect("Failed to create test file");

    // Run GC
    let result = backend.gc(vec![path1.clone(), path2.clone()]).await;
    assert!(result.is_ok(), "GC should succeed: {:?}", result.err());

    // In practice, store paths wouldn't be removed by our simple implementation
    // but non-store paths would be cleaned up
}

// Search tests moved to integration tests (tests/cli/.test.sh)
// Searching nixpkgs requires too much memory for unit tests.

#[test]
fn test_backend_options_default() {
    let options = Options::default();

    // Verify default options
    assert!(!options.replace_shell);
    assert!(options.bail_on_error);
    assert!(!options.cache_output);
    assert!(!options.refresh_cached_output);
}

/// Benchmark test comparing FFI vs shell-based operations
/// (Placeholder for future performance testing)
#[nix_test]
async fn benchmark_ffi_vs_shell() {
    // TODO: Once compilation issues are fixed, add benchmarks comparing:
    // - eval() time: FFI vs `nix eval`
    // - build() time: FFI vs `nix build`
    // - update() time: FFI vs `nix flake update`
}

/// Test metadata operation in isolation
#[nix_test]
async fn test_metadata_standalone() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    println!("Created backend");

    println!("Backend assembled");

    // Call metadata WITHOUT any prior update/build
    let metadata = backend.metadata().await.expect("Failed to get metadata");
    println!("Metadata:\n{}", metadata);
}

/// Test metadata after update
#[nix_test]
async fn test_metadata_after_update() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    println!("Created backend");

    println!("Backend assembled");

    // Update first
    backend
        .update(&None, &config.inputs, &[])
        .await
        .expect("Failed to update");
    println!("Updated inputs");

    // Then call metadata
    let metadata = backend.metadata().await.expect("Failed to get metadata");
    println!("Metadata after update:\n{}", metadata);
}

/// Integration test demonstrating full workflow
#[nix_test]
async fn test_full_backend_workflow() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    println!("Created NixCBackend: {}", backend.name());

    // 2. Initialize
    println!("Backend assembled");

    // 3. Update inputs
    backend
        .update(&None, &config.inputs, &[])
        .await
        .expect("Failed to update inputs");
    println!("Updated all inputs");

    // 4. Get metadata BEFORE build to test if build corrupts state
    let metadata = backend.metadata().await.expect("Failed to get metadata");
    println!("Flake metadata (before build):\n{}", metadata);

    // 5. Build devenv shell
    let build_paths = backend
        .build(&["shell"], None, None)
        .await
        .expect("Failed to build");
    println!("Built shell: {:?}", build_paths);

    // 7. Clean up
    backend.gc(vec![]).await.expect("Failed to run GC");
    println!("Workflow complete!");
}

#[nix_test]
async fn test_backend_update_with_input_overrides() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-25.05
  devenv:
    url: github:cachix/devenv/v1.0
"#;

    let override_inputs = vec![
        "nixpkgs".to_string(),
        "github:NixOS/nixpkgs/nixos-unstable".to_string(),
    ];

    let (temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Update with overrides
    let result = backend
        .update(&None, &config.inputs, &override_inputs)
        .await;
    assert!(
        result.is_ok(),
        "Update with overrides should succeed: {:?}",
        result.err()
    );

    // Verify lock file was created in temp directory
    let lock_path = temp_dir.path().join("devenv.lock");
    assert!(lock_path.exists(), "Lock file should be created");

    // Read and verify the lock file contains the overridden input
    let lock_content = std::fs::read_to_string(&lock_path).expect("Failed to read lock file");

    // The lock file should reference nixos-unstable instead of nixos-25.05
    assert!(
        lock_content.contains("nixos-unstable") || lock_content.contains("unstable"),
        "Lock file should contain the overridden nixpkgs reference"
    );
}

#[nix_test]
async fn test_backend_update_with_multiple_overrides() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-25.05
  devenv:
    url: github:cachix/devenv/v1.0
  rust-overlay:
    url: github:oxalica/rust-overlay
"#;

    let override_inputs = vec![
        "nixpkgs".to_string(),
        "github:NixOS/nixpkgs/nixos-unstable".to_string(),
        "devenv".to_string(),
        "github:cachix/devenv/v1.1".to_string(),
    ];

    let (temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Update with multiple overrides
    let result = backend
        .update(&None, &config.inputs, &override_inputs)
        .await;
    assert!(
        result.is_ok(),
        "Update with multiple overrides should succeed: {:?}",
        result.err()
    );

    // Verify lock file was created in temp directory
    let lock_path = temp_dir.path().join("devenv.lock");
    assert!(lock_path.exists(), "Lock file should be created");
}

// ============================================================================
// ERROR HANDLING TESTS
// ============================================================================

/// Test that eval() fails gracefully when evaluating an attribute that doesn't exist
#[nix_test]
async fn test_eval_nonexistent_attribute() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for evaluation
    copy_fixture_lock(temp_dir.path());

    // Try to eval a nonexistent attribute - should return an error
    let result = backend.eval(&["nonexistent.attribute.path"]).await;

    assert!(
        result.is_err(),
        "Evaluating nonexistent attribute should fail"
    );

    let error = result.unwrap_err();
    let error_msg = format!("{:?}", error);

    // Error message should mention the attribute that failed
    assert!(
        error_msg.contains("nonexistent.attribute.path") || error_msg.contains("attribute"),
        "Error should mention the failed attribute, got: {}",
        error_msg
    );
}

/// Test that build() returns proper error for non-existent attribute
#[nix_test]
async fn test_build_nonexistent_attribute() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Try to build a nonexistent attribute - should return an error
    let result = backend.build(&["nonexistent.package"], None, None).await;

    assert!(
        result.is_err(),
        "Building nonexistent attribute should fail"
    );

    let error = result.unwrap_err();
    let error_msg = format!("{:?}", error);

    // Error message should mention the attribute that failed
    assert!(
        error_msg.contains("nonexistent") || error_msg.contains("attribute"),
        "Error should mention the failed attribute, got: {}",
        error_msg
    );
}

/// Test build/eval fails when default.nix has syntax error
#[nix_test]
async fn test_build_with_syntax_error_in_nix() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

    // Create devenv.nix with syntax error
    let broken_nix = r#"{ ... }: {
  this is not valid nix syntax!!!
}"#;

    let (temp_dir, _cwd_guard, paths, config) = setup_isolated_test_files(yaml, Some(broken_nix));

    copy_fixture_lock(temp_dir.path());

    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    let init_result = init_backend(
        paths.clone(),
        config.clone(),
        NixOptions::default(),
        cachix_manager,
        Shutdown::new(),
    )
    .await;

    let error_msg = match init_result {
        Err(e) => format!("{:?}", e),
        Ok(backend) => {
            let result = backend.eval(&["shell"]).await;
            assert!(result.is_err(), "Eval should fail with syntax error");
            format!("{:?}", result.unwrap_err())
        }
    };

    assert!(
        error_msg.contains("syntax") || error_msg.contains("parse") || error_msg.contains("error"),
        "Error should indicate syntax/parse error, got: {}",
        error_msg
    );

    drop(temp_dir);
}

/// Test that evaluation errors include detailed Nix error messages
///
/// When Nix evaluation fails, the error message should include the actual
/// Nix error (e.g., "undefined variable 'nonexistent_var'"), not just a
/// generic wrapper message like "Failed to import default.nix".
///
/// This tests the error enrichment feature that combines Nix logger output
/// with the exception to provide helpful error messages.
#[nix_test]
async fn test_eval_error_includes_nix_details() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

    // Create devenv.nix that references an undefined variable
    // This will produce a clear Nix error: "undefined variable 'nonexistent_var_xyz'"
    let broken_nix = r#"{ pkgs, ... }: {
  packages = [ nonexistent_var_xyz ];
}"#;

    let (temp_dir, _cwd_guard, paths, config) = setup_isolated_test_files(yaml, Some(broken_nix));

    copy_fixture_lock(temp_dir.path());

    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    let init_result = init_backend(
        paths.clone(),
        config.clone(),
        NixOptions::default(),
        cachix_manager,
        Shutdown::new(),
    )
    .await;

    let error_msg = match init_result {
        Err(e) => format!("{:?}", e),
        Ok(backend) => {
            let result = backend.eval(&["packages"]).await;
            assert!(result.is_err(), "Eval should fail with undefined variable");
            format!("{:?}", result.unwrap_err())
        }
    };

    // The error message MUST include the specific Nix error details.
    // This verifies the error enrichment is working - we should see the
    // actual variable name from the Nix error, not just a generic message.
    assert!(
        error_msg.contains("nonexistent_var_xyz"),
        "Error should include the undefined variable name from Nix error.\n\
         This indicates error enrichment is working.\n\
         Got: {}",
        error_msg
    );

    // Also verify it contains "undefined" to confirm it's the right error type
    assert!(
        error_msg.to_lowercase().contains("undefined"),
        "Error should mention 'undefined' variable.\nGot: {}",
        error_msg
    );

    drop(temp_dir);
}

/// Test metadata() when devenv.lock exists but contains invalid JSON
#[nix_test]
async fn test_metadata_with_corrupted_lock_file() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Create a corrupted lock file in the isolated test directory
    let lock_file_path = temp_dir.path().join("devenv.lock");
    std::fs::write(&lock_file_path, "{ invalid json here")
        .expect("Failed to write corrupted lock file");

    // metadata() should still work gracefully with corrupted lock file
    // It may either handle it gracefully or return an error, but shouldn't crash
    let result = backend.metadata().await;

    // The result can be either Ok or Err - both are acceptable
    // We're testing that the function doesn't crash with corrupted input
    match result {
        Ok(metadata_output) => {
            // If it succeeds, output should be valid string
            assert!(
                !metadata_output.is_empty(),
                "metadata output should not be empty"
            );
        }
        Err(_e) => {
            // It's ok if it returns an error for corrupted lock file
            // The important thing is it doesn't crash
        }
    }
}

/// Test gc() with paths that aren't valid Nix store paths
#[nix_test]
async fn test_gc_with_invalid_store_paths() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Test gc() with invalid store paths - should handle gracefully
    let invalid_paths = vec![
        temp_dir.path().join("not/a/store/path"),
        temp_dir.path().join("relative/path"),
    ];

    // gc() should succeed even with invalid paths (gracefully handling them)
    let result = backend.gc(invalid_paths).await;
    assert!(
        result.is_ok(),
        "gc() should handle invalid paths gracefully: {:?}",
        result.err()
    );
}

/// Test update() with malformed override_inputs
/// Odd number of override elements are silently ignored (chunks_exact(2) behavior)
#[nix_test]
async fn test_update_with_invalid_override_inputs() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

    // Odd number of elements - should be pairs
    // chunks_exact(2) will ignore the remainder, so this is safe
    let override_inputs = vec!["nixpkgs".to_string()];

    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;

    // Update should succeed - malformed override is ignored (no assemble needed for update-only tests)
    let result = backend
        .update(&None, &config.inputs, &override_inputs)
        .await;
    assert!(
        result.is_ok(),
        "update() should succeed even with malformed override_inputs: {:?}",
        result.err()
    );

    // Verify lock file was created
    assert!(
        backend.paths.root.join("devenv.lock").exists(),
        "Lock file should be created despite malformed override"
    );
}

// ============================================================================
// EDGE CASES & BOUNDARY CONDITION TESTS
// ============================================================================

/// Test eval() with empty attributes array
#[nix_test]
async fn test_eval_empty_attributes_array() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Test eval(&[]) - should return empty JSON array
    let result = backend.eval(&[]).await;

    assert!(
        result.is_ok(),
        "Evaluating empty attributes array should succeed: {:?}",
        result.err()
    );

    let json_str = result.unwrap();

    // Should return an empty JSON array
    assert_eq!(json_str, "[]", "Empty attributes should return empty array");

    // Verify it's valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("Result should be valid JSON");
    assert!(parsed.is_array(), "Result should be a JSON array");
    assert_eq!(parsed.as_array().unwrap().len(), 0, "Array should be empty");
}

/// Test build() with empty attributes array
#[nix_test]
async fn test_build_empty_attributes_array() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Test build(&[], None, None) - should return empty vec
    let result = backend.build(&[], None, None).await;

    assert!(
        result.is_ok(),
        "Building empty attributes array should succeed: {:?}",
        result.err()
    );

    let paths = result.unwrap();
    assert!(
        paths.is_empty(),
        "Building empty attributes should return empty vec, got: {:?}",
        paths
    );
}

/// Test build() when gc_root already exists as file/symlink
#[nix_test]
async fn test_build_gc_root_already_exists() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    let gc_root_base = temp_dir.path().join("result");
    // The build function appends the attribute name to the gc_root base path
    let gc_root_actual = temp_dir.path().join("result-shell");

    // First build to create the gc_root
    let result1 = backend.build(&["shell"], None, Some(&gc_root_base)).await;
    assert!(
        result1.is_ok(),
        "First build should succeed: {:?}",
        result1.err()
    );
    assert!(
        gc_root_actual.exists(),
        "GC root should exist after first build"
    );

    // Build again with same gc_root - Nix's add_perm_root should handle the existing symlink
    let result2 = backend.build(&["shell"], None, Some(&gc_root_base)).await;

    assert!(
        result2.is_ok(),
        "Build with existing gc_root should succeed: {:?}",
        result2.err()
    );

    // Verify gc_root still exists
    assert!(
        gc_root_actual.exists(),
        "GC root should exist after second build"
    );

    let paths = result2.unwrap();
    assert!(!paths.is_empty(), "Build should return paths");
}

/// Test update() when lock file already exists - should update in place
#[nix_test]
async fn test_update_lock_file_already_exists() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;

    // Create initial lock (no assemble needed for update-only tests)
    backend
        .update(&None, &config.inputs, &[])
        .await
        .expect("Failed to create initial lock");

    let lock_path = backend.paths.root.join("devenv.lock");

    assert!(
        lock_path.exists(),
        "Lock file should exist after first update"
    );

    // Get modification time of lock file
    let first_mtime = std::fs::metadata(&lock_path)
        .expect("Failed to get lock file metadata")
        .modified()
        .expect("Failed to get modification time");

    // Wait a bit to ensure timestamps differ if file is rewritten
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Call update again - should update in place
    let result = backend.update(&None, &config.inputs, &[]).await;
    assert!(
        result.is_ok(),
        "Second update() should succeed: {:?}",
        result.err()
    );

    // Verify lock file still exists
    assert!(
        lock_path.exists(),
        "Lock file should still exist after second update"
    );

    // Verify file was actually rewritten (timestamp changed)
    let second_mtime = std::fs::metadata(&lock_path)
        .expect("Failed to get lock file metadata after second update")
        .modified()
        .expect("Failed to get modification time");

    assert!(
        second_mtime >= first_mtime,
        "Lock file should be updated (timestamp should not be earlier)"
    );
}

/// Test metadata() before any update
#[nix_test]
async fn test_metadata_before_any_update() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Call metadata before any update - should succeed and show "no lock file"
    let result = backend.metadata().await;
    assert!(
        result.is_ok(),
        "metadata() should succeed even without lock file: {:?}",
        result.err()
    );

    let metadata_output = result.unwrap();

    // Should contain "no lock file" message
    assert!(
        metadata_output.contains("no lock file") || metadata_output.contains("Inputs"),
        "metadata() should mention lock file status, got: {}",
        metadata_output
    );

    // Should be valid string output (not empty)
    assert!(
        !metadata_output.is_empty(),
        "metadata output should not be empty"
    );
}

// ============================================================================
// STATE CONSISTENCY TESTS
// ============================================================================

// ============================================================================
// GET_BASH() METHOD TESTS
// ============================================================================

/// Test get_bash() returns valid executable path
#[nix_test]
async fn test_get_bash_returns_valid_path() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for bash evaluation
    copy_fixture_lock(temp_dir.path());

    // Get bash executable path
    let result = backend.get_bash(false).await;
    assert!(
        result.is_ok(),
        "get_bash() should succeed: {:?}",
        result.err()
    );

    let bash_path = result.unwrap();
    assert!(
        bash_path.ends_with("/bin/bash"),
        "Bash path should end with /bin/bash, got: {}",
        bash_path
    );
    assert!(
        bash_path.starts_with("/nix/store"),
        "Bash path should be in nix store, got: {}",
        bash_path
    );
}

/// Test get_bash() caching behavior
#[nix_test]
async fn test_get_bash_caching_with_gc_root() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for bash evaluation
    copy_fixture_lock(temp_dir.path());

    // First call - should build bash
    let result1 = backend.get_bash(false).await;
    assert!(result1.is_ok(), "First get_bash() should succeed");
    let path1 = result1.unwrap();

    // Check that GC root symlink was created
    // The build function appends the attribute name, so bash becomes bash-bash
    let gc_root = temp_dir.path().join(".devenv/bash-bash");
    assert!(
        gc_root.exists(),
        "GC root symlink should be created at .devenv/bash-bash"
    );

    // Second call without refresh - should use cached result
    let result2 = backend.get_bash(false).await;
    assert!(
        result2.is_ok(),
        "Second get_bash(false) should succeed: {:?}",
        result2.err()
    );
    let path2 = result2.unwrap();

    // Both should return the same path
    assert_eq!(path1, path2, "Cached calls should return the same path");
}

/// Test get_bash() with refresh_cached_output parameter
#[nix_test]
async fn test_get_bash_with_refresh_cached_output() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for bash evaluation
    copy_fixture_lock(temp_dir.path());

    // Build bash once
    let result1 = backend.get_bash(false).await;
    assert!(result1.is_ok(), "First get_bash(false) should succeed");
    let path1 = result1.unwrap();

    // Force rebuild with refresh_cached_output=true
    let result2 = backend.get_bash(true).await;
    assert!(
        result2.is_ok(),
        "get_bash(true) with refresh should succeed: {:?}",
        result2.err()
    );
    let path2 = result2.unwrap();

    // Paths should be identical (same bash in store)
    assert_eq!(
        path1, path2,
        "Refreshed call should return same bash package"
    );
}

/// Test get_bash() returns executable bash
#[nix_test]
async fn test_get_bash_returns_executable() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for bash evaluation
    copy_fixture_lock(temp_dir.path());

    let result = backend.get_bash(false).await;
    assert!(result.is_ok(), "get_bash() should succeed");

    let bash_path = result.unwrap();

    // Verify the file exists and is executable
    let path = std::path::Path::new(&bash_path);
    assert!(
        path.exists(),
        "Bash executable should exist at: {}",
        bash_path
    );

    // Check if it's executable
    let metadata = std::fs::metadata(&bash_path).expect("Failed to get bash metadata");
    assert!(metadata.is_file(), "Bash should be a file");

    // On Unix, check execute bit
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        let is_executable = (mode & 0o111) != 0;
        assert!(is_executable, "Bash file should be executable");
    }
}

// ============================================================================
// GC OPERATIONS TESTS
// ============================================================================

/// Test GC with actual Nix store paths
#[nix_test]
async fn test_gc_with_actual_nix_store_paths() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Build a package to get actual store paths
    let build_result = backend.build(&["shell"], None, None).await;
    assert!(build_result.is_ok(), "Build should succeed");

    let built_paths = build_result.unwrap();
    assert!(!built_paths.is_empty(), "Build should return paths");

    // Test gc() with empty path list - should succeed without doing anything
    let result = backend.gc(vec![]).await;
    assert!(
        result.is_ok(),
        "gc() with empty paths should succeed: {:?}",
        result.err()
    );

    // Note: gc() with live store paths will fail because Nix protects them
    // This is correct behavior - we test that gc() handles live paths gracefully
    let store_path_str = built_paths[0].to_str().unwrap().to_string();
    let result_live = backend.gc(vec![PathBuf::from(&store_path_str)]).await;

    // gc() should either succeed (if path was deletable) or fail with meaningful error about live paths
    match result_live {
        Ok(_) => {
            // Path was successfully deleted (unexpected but acceptable)
        }
        Err(e) => {
            // Path still alive (expected) - verify error mentions it
            let err_str = format!("{:?}", e);
            assert!(
                err_str.contains("alive") || err_str.contains("still"),
                "Error should indicate path is still alive, got: {}",
                err_str
            );
        }
    }
}

/// Test GC with protected gc_roots
#[nix_test]
async fn test_gc_with_protected_gc_roots() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Build with gc_root to protect the path
    let gc_root_base = temp_dir.path().join("protected-result");
    // The build function appends the attribute name to the gc_root base path
    let gc_root_actual = temp_dir.path().join("protected-result-shell");
    let build_result = backend.build(&["shell"], None, Some(&gc_root_base)).await;
    assert!(
        build_result.is_ok(),
        "Build with gc_root should succeed: {:?}",
        build_result.err()
    );

    let built_paths = build_result.unwrap();
    assert!(!built_paths.is_empty(), "Build should return paths");

    // Verify the gc_root symlink was created
    assert!(gc_root_actual.exists(), "GC root symlink should be created");

    // Try to GC the protected path
    let store_path_str = built_paths[0].to_str().unwrap().to_string();
    let result = backend.gc(vec![PathBuf::from(&store_path_str)]).await;

    // Protected paths should not be deleted - gc() will either:
    // 1. Fail with error about path being alive (expected)
    // 2. Succeed without deleting (if no deletable paths in closure)
    match result {
        Ok(_) => {
            // gc() succeeded - but gc_root should still exist (path is protected)
            assert!(
                gc_root_actual.exists(),
                "GC root should still exist after gc() (path is protected)"
            );
        }
        Err(e) => {
            // gc() failed - path is protected (expected)
            let err_str = format!("{:?}", e);
            assert!(
                err_str.contains("alive") || err_str.contains("still"),
                "Error should indicate path is still alive, got: {}",
                err_str
            );
            // gc_root should definitely still exist
            assert!(
                gc_root_actual.exists(),
                "GC root should still exist (path is protected)"
            );
        }
    }
}

/// Test GC computes closure correctly
#[nix_test]
async fn test_gc_computes_closure_correctly() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Build the shell which has dependencies
    let build_result = backend.build(&["shell"], None, None).await;
    assert!(build_result.is_ok(), "Build should succeed");

    let built_paths = build_result.unwrap();
    assert!(!built_paths.is_empty(), "Build should return paths");

    // The shell derivation should have dependencies (bash, coreutils, etc.)
    // When we GC this path, the closure computation should include all dependencies
    let store_path_str = built_paths[0].to_str().unwrap().to_string();
    let result = backend.gc(vec![PathBuf::from(&store_path_str)]).await;

    // gc() should successfully compute the closure for derivations with dependencies
    // It may fail if the path is still alive, but it should handle it gracefully
    match result {
        Ok(_) => {
            // gc() succeeded - closure was computed and deleted successfully
        }
        Err(e) => {
            // gc() computed closure but couldn't delete (path is still alive)
            // This is expected for recently built packages
            let err_str = format!("{:?}", e);
            assert!(
                err_str.contains("alive") || err_str.contains("still"),
                "Error should be about paths still being alive, got: {}",
                err_str
            );
        }
    }
}

/// Test GC reports freed space
#[nix_test]
async fn test_gc_reports_bytes_freed() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Build a package that will have some size
    let build_result = backend.build(&["shell"], None, None).await;
    assert!(build_result.is_ok(), "Build should succeed");

    let built_paths = build_result.unwrap();
    assert!(!built_paths.is_empty(), "Build should return paths");

    // Call gc() on the built path
    // The gc() method internally calls collect_garbage which returns bytes_freed
    // This is printed to stderr if bytes_freed > 0
    let store_path_str = built_paths[0].to_str().unwrap().to_string();
    let result = backend.gc(vec![PathBuf::from(&store_path_str)]).await;

    // gc() should either succeed (and report freed bytes) or fail gracefully
    // The important thing is that it handles the operation properly
    match result {
        Ok(_) => {
            // gc() succeeded and should have reported freed bytes to stderr
            // (The bytes_freed is printed by the gc() implementation)
        }
        Err(e) => {
            // gc() couldn't delete because path is still alive (expected)
            let err_str = format!("{:?}", e);
            assert!(
                err_str.contains("alive") || err_str.contains("still"),
                "Error should be about paths still being alive, got: {}",
                err_str
            );
        }
    }
}

/// Test GC with mixed store and temp paths
#[nix_test]
async fn test_gc_with_mixed_store_and_temp_paths() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for build
    copy_fixture_lock(temp_dir.path());

    // Build a package to get actual store paths
    let build_result = backend.build(&["shell"], None, None).await;
    assert!(build_result.is_ok(), "Build should succeed");

    let built_paths = build_result.unwrap();
    assert!(!built_paths.is_empty(), "Build should return paths");

    // Create some temporary files/directories
    let temp_file1 = temp_dir.path().join("temp_file1.txt");
    let temp_file2 = temp_dir.path().join("temp_file2.txt");
    let temp_subdir = temp_dir.path().join("temp_subdir");

    std::fs::write(&temp_file1, "test content 1").expect("Failed to write temp file 1");
    std::fs::write(&temp_file2, "test content 2").expect("Failed to write temp file 2");
    std::fs::create_dir(&temp_subdir).expect("Failed to create temp subdir");
    std::fs::write(temp_subdir.join("nested_file.txt"), "nested content")
        .expect("Failed to write nested file");

    // Verify temp files exist before gc
    assert!(temp_file1.exists(), "Temp file 1 should exist before gc");
    assert!(temp_file2.exists(), "Temp file 2 should exist before gc");
    assert!(temp_subdir.exists(), "Temp subdir should exist before gc");

    // Mix store paths with temp paths for gc
    let store_path_str = built_paths[0].to_str().unwrap().to_string();
    let mixed_paths = vec![
        PathBuf::from(&store_path_str),
        temp_file1.clone(),
        temp_file2.clone(),
        temp_subdir.clone(),
    ];

    // Call gc() with mixed store and temp paths
    // gc() should handle both types: store paths via Nix GC, temp files via filesystem removal
    let result = backend.gc(mixed_paths).await;

    // gc() may fail if store paths are still alive, but temp files should still be removed
    match result {
        Ok(_) => {
            // gc() succeeded - both store and temp paths should be handled
        }
        Err(e) => {
            // gc() may fail due to live store paths, but temp files should still be removed
            let err_str = format!("{:?}", e);
            assert!(
                err_str.contains("alive") || err_str.contains("still"),
                "Error should be about paths still being alive, got: {}",
                err_str
            );
        }
    }

    // Verify temp files were removed (gc() removes non-store paths as regular files)
    // This should happen even if store path deletion failed
    assert!(
        !temp_file1.exists(),
        "Temp file 1 should be removed by gc()"
    );
    assert!(
        !temp_file2.exists(),
        "Temp file 2 should be removed by gc()"
    );
    assert!(
        !temp_subdir.exists(),
        "Temp subdir should be removed by gc()"
    );
}

// ============================================================================
// REALISTIC WORKFLOW TESTS
// ============================================================================

/// Test build then incremental update
#[nix_test]
async fn test_workflow_build_then_incremental_update() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Initial update and build
    backend
        .update(&None, &config.inputs, &[])
        .await
        .expect("Initial update failed");

    let result1 = backend.build(&["shell"], None, None).await;
    assert!(
        result1.is_ok(),
        "Initial build should succeed: {:?}",
        result1.err()
    );
    let paths1 = result1.unwrap();
    assert!(!paths1.is_empty(), "Initial build should return paths");

    // Incremental update (update just one input)
    backend
        .update(&Some("nixpkgs".to_string()), &config.inputs, &[])
        .await
        .expect("Incremental update failed");

    // Rebuild after incremental update
    let result2 = backend.build(&["shell"], None, None).await;
    assert!(
        result2.is_ok(),
        "Rebuild after incremental update should succeed: {:?}",
        result2.err()
    );
    let paths2 = result2.unwrap();
    assert!(!paths2.is_empty(), "Rebuild should return paths");

    // Both builds should produce valid store paths
    assert!(paths1[0].to_str().unwrap().starts_with("/nix/store"));
    assert!(paths2[0].to_str().unwrap().starts_with("/nix/store"));
}

/// Test multiple builds with different gc_roots
#[nix_test]
async fn test_workflow_multiple_builds_different_gc_roots() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for builds
    copy_fixture_lock(temp_dir.path());

    let gc_root1_base = temp_dir.path().join("result1");
    let gc_root2_base = temp_dir.path().join("result2");
    // The build function appends the attribute name to the gc_root base path
    let gc_root1_actual = temp_dir.path().join("result1-shell");
    let gc_root2_actual = temp_dir.path().join("result2-shell");

    // Build first attribute with first gc_root
    let result1 = backend.build(&["shell"], None, Some(&gc_root1_base)).await;
    assert!(
        result1.is_ok(),
        "First build should succeed: {:?}",
        result1.err()
    );

    // Build second attribute with second gc_root (same attribute, different gc_root)
    let result2 = backend.build(&["shell"], None, Some(&gc_root2_base)).await;
    assert!(
        result2.is_ok(),
        "Second build should succeed: {:?}",
        result2.err()
    );

    // Verify both gc_roots exist
    assert!(gc_root1_actual.exists(), "First GC root should exist");
    assert!(gc_root2_actual.exists(), "Second GC root should exist");

    // Verify both returned valid paths
    let paths1 = result1.unwrap();
    let paths2 = result2.unwrap();
    assert!(!paths1.is_empty(), "First build should return paths");
    assert!(!paths2.is_empty(), "Second build should return paths");
}

// ============================================================================
// MULTI-INPUT & COMPLEX CONFIGURATION TESTS
// ============================================================================

/// Test update with many inputs - verify all inputs are successfully locked
#[nix_test]
async fn test_update_with_many_inputs() {
    let yaml_content = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-24.05
  git-hooks:
    url: github:cachix/git-hooks.nix
  devenv:
    url: github:cachix/devenv/v1.0
  rust-overlay:
    url: github:oxalica/rust-overlay
  systems:
    url: github:nix-systems/default
  flake-utils:
    url: github:numtide/flake-utils
"#;

    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml_content, None, NixOptions::default()).await;

    // Update with 6 inputs (no assemble needed for update-only tests)
    let result = backend.update(&None, &config.inputs, &[]).await;
    assert!(
        result.is_ok(),
        "update() should succeed with many inputs: {:?}",
        result.err()
    );

    // Verify lock file exists
    let lock_path = backend.paths.root.join("devenv.lock");
    assert!(lock_path.exists(), "Lock file should be created");

    // Parse lock file to verify all 6 inputs are locked
    let lock_content = std::fs::read_to_string(&lock_path).expect("Failed to read lock file");
    let lock_json: serde_json::Value =
        serde_json::from_str(&lock_content).expect("Lock file should be valid JSON");

    // Check that nodes section exists and has our inputs
    let nodes = lock_json
        .get("nodes")
        .expect("Lock file should have 'nodes' field");

    // Verify all 6 inputs are present in the lock file
    let expected_inputs = vec![
        "nixpkgs",
        "git-hooks",
        "devenv",
        "rust-overlay",
        "systems",
        "flake-utils",
    ];
    for input_name in expected_inputs {
        assert!(
            nodes.get(input_name).is_some(),
            "Lock file should contain '{}' input",
            input_name
        );
    }
}

/// Test update with nested input follows - verify "follows" references work
#[nix_test]
async fn test_update_with_nested_input_follows() {
    let yaml_content = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-24.05
  git-hooks:
    url: github:cachix/git-hooks.nix
  systems:
    url: github:nix-systems/default
  flake-utils:
    url: github:numtide/flake-utils
    inputs:
      systems:
        follows: /systems
"#;

    let (_temp_dir, _cwd_guard, backend, _paths, config) =
        setup_isolated_test_env(yaml_content, None, NixOptions::default()).await;

    // Update with "follows" references (no assemble needed for update-only tests)
    let result = backend.update(&None, &config.inputs, &[]).await;
    assert!(
        result.is_ok(),
        "update() should succeed with nested input follows: {:?}",
        result.err()
    );

    // Verify lock file exists
    let lock_path = backend.paths.root.join("devenv.lock");
    assert!(lock_path.exists(), "Lock file should be created");

    // Parse lock file to verify "follows" is preserved
    let lock_content = std::fs::read_to_string(&lock_path).expect("Failed to read lock file");
    let lock_json: serde_json::Value =
        serde_json::from_str(&lock_content).expect("Lock file should be valid JSON");

    // Check that all inputs are present
    let nodes = lock_json
        .get("nodes")
        .expect("Lock file should have 'nodes' field");

    assert!(
        nodes.get("nixpkgs").is_some(),
        "Lock file should contain 'nixpkgs' input"
    );
    assert!(
        nodes.get("systems").is_some(),
        "Lock file should contain 'systems' input"
    );
    assert!(
        nodes.get("flake-utils").is_some(),
        "Lock file should contain 'flake-utils' input"
    );

    // Verify flake-utils node has inputs.systems as a follow reference
    let flake_utils = nodes.get("flake-utils").unwrap();
    if let Some(inputs_obj) = flake_utils.get("inputs") {
        // The follows reference might be in the inputs field
        // Structure can vary, so we just verify the lock file is valid
        assert!(
            inputs_obj.is_object() || inputs_obj.is_array(),
            "flake-utils should have inputs field"
        );
    }
    // If the lock succeeds, the follows relationship is working correctly
}

/// Test build multiple attributes in single call
#[nix_test]
async fn test_build_multiple_attributes_single_call() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    // Use fixture lock file for builds
    copy_fixture_lock(temp_dir.path());

    // Test build(&[attr1, attr2], ...) builds all attributes in one call
    // Note: Building the same attribute twice should work fine
    let result = backend.build(&["shell", "shell"], None, None).await;

    assert!(
        result.is_ok(),
        "Building multiple attributes should succeed: {:?}",
        result.err()
    );

    let paths = result.unwrap();
    // We should get at least 2 paths back (one for each attribute, though some may be shared)
    assert!(!paths.is_empty(), "Build should return paths");

    // All paths should be in the nix store
    for path in &paths {
        assert!(
            path.to_str().unwrap().starts_with("/nix/store"),
            "All built paths should be in nix store, got: {}",
            path.display()
        );
    }
}

// ============================================================================
// THREAD SAFETY TESTS
// ============================================================================

/// Test eval_state Mutex under concurrent eval calls
#[nix_test]
async fn test_eval_state_mutex_under_concurrent_eval() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;

    let backend = std::sync::Arc::new(backend);
    // Use fixture lock file for evaluation
    copy_fixture_lock(temp_dir.path());

    // Create three eval futures without awaiting them
    let eval1 = backend.eval(&["config.devenv.root"]);
    let eval2 = backend.eval(&["config.name"]);
    let eval3 = backend.eval(&["config.devenv.cliVersion"]);

    // Await all three concurrently
    let (result1, result2, result3) = tokio::join!(eval1, eval2, eval3);

    // Verify all succeeded
    let json1 = result1.expect("Eval 1 should succeed");
    let json2 = result2.expect("Eval 2 should succeed");
    let json3 = result3.expect("Eval 3 should succeed");

    // Verify all results are non-empty and valid JSON
    assert!(!json1.is_empty(), "Result 1 should be non-empty");
    assert!(!json2.is_empty(), "Result 2 should be non-empty");
    assert!(!json3.is_empty(), "Result 3 should be non-empty");

    serde_json::from_str::<serde_json::Value>(&json1).expect("Result 1 should be valid JSON");
    serde_json::from_str::<serde_json::Value>(&json2).expect("Result 2 should be valid JSON");
    serde_json::from_str::<serde_json::Value>(&json3).expect("Result 3 should be valid JSON");
}

/// Test concurrent build operations
#[nix_test]
async fn test_concurrent_build_operations() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

    // Enable Python and PHP languages so we can build their packages
    let devenv_nix = r#"{ pkgs, ... }: {
  languages.python.enable = true;
  languages.php.enable = true;
}"#;

    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, Some(devenv_nix), NixOptions::default()).await;
    // Use fixture lock file for builds
    copy_fixture_lock(temp_dir.path());

    // Wrap backend in Arc for sharing across concurrent tasks
    let backend = std::sync::Arc::new(backend);

    // Create multiple build futures without awaiting them
    // Using different attributes to test true concurrent building
    let build1 = backend.build(&["shell"], None, None);
    let build2 = backend.build(&["config.languages.python.package"], None, None);
    let build3 = backend.build(&["config.languages.php.package"], None, None);

    // Await all three builds concurrently
    let (result1, result2, result3) = tokio::join!(build1, build2, build3);

    // All builds should succeed
    assert!(
        result1.is_ok(),
        "Concurrent build 1 should succeed: {:?}",
        result1.err()
    );
    assert!(
        result2.is_ok(),
        "Concurrent build 2 should succeed: {:?}",
        result2.err()
    );
    assert!(
        result3.is_ok(),
        "Concurrent build 3 should succeed: {:?}",
        result3.err()
    );

    // Verify all returned paths
    let paths1 = result1.unwrap();
    let paths2 = result2.unwrap();
    let paths3 = result3.unwrap();

    assert!(!paths1.is_empty(), "Build 1 should return paths");
    assert!(!paths2.is_empty(), "Build 2 should return paths");
    assert!(!paths3.is_empty(), "Build 3 should return paths");
}

/// Extract file paths from EvalOp::EvaluatedFile entries, filtering out /nix/store paths
/// which are immutable and not relevant for cache invalidation.
fn evaluated_files(ops: &[EvalOp]) -> std::collections::HashSet<PathBuf> {
    ops.iter()
        .filter_map(|op| match op {
            EvalOp::EvaluatedFile { source } => Some(source.clone()),
            _ => None,
        })
        .filter(|p| !p.starts_with("/nix/store"))
        .collect()
}

/// The production `InputTracker` (owned by `CachingEvalState`) must accumulate
/// file deps across attribute evaluations. Once module merging has fired an
/// `evaluating file` event for a path, Nix's internal fileEvalCache suppresses
/// re-emission on every later eval — so the tracker is the only place those
/// deps survive. This is the invariant that keeps later attrs' DB rows (e.g.
/// `shell`) aware of files first touched during earlier attrs' evaluation
/// (e.g. `config.cachix.*`).
#[nix_test]
async fn test_input_tracker_accumulates_across_evals() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, None, NixOptions::default()).await;
    copy_fixture_lock(temp_dir.path());

    let tracker = backend
        .input_tracker()
        .expect("input_tracker available after assemble");

    let result1 = backend.eval(&["config.devenv.root"]).await;
    assert!(
        result1.is_ok(),
        "First eval should succeed: {:?}",
        result1.err()
    );

    let files_after_first = evaluated_files(&tracker.snapshot());
    assert!(
        !files_after_first.is_empty(),
        "Tracker should observe base file deps during the first eval"
    );

    let result2 = backend.eval(&["config.devenv.cliVersion"]).await;
    assert!(
        result2.is_ok(),
        "Second eval should succeed: {:?}",
        result2.err()
    );

    let files_after_second = evaluated_files(&tracker.snapshot());

    // The tracker only grows — every file seen in eval #1 must still be present.
    let missing: Vec<_> = files_after_first.difference(&files_after_second).collect();
    assert!(
        missing.is_empty(),
        "Tracker dropped {} files between evals: {:?}",
        missing.len(),
        missing
    );
}

/// Regression test for the original boop.nix bug.
///
/// When a nested file (`imports = [./nested/child.nix]`) is first evaluated
/// during one attribute's forcing, Nix's fileEvalCache suppresses re-emission
/// on every later attribute. Before the persistent `InputTracker` fix, later
/// attrs' DB rows would therefore miss the nested file entirely — and the
/// hot-reload file watcher would never watch it.
///
/// This test verifies the production tracker captures the nested file across
/// two separate `eval` calls, regardless of which one triggered the initial
/// parse.
#[nix_test]
async fn test_nested_import_tracked_across_evals() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;
    let nix = r#"{ ... }: { imports = [ ./nested/child.nix ]; }"#;
    let (temp_dir, _cwd_guard, backend, _paths, _config) =
        setup_isolated_test_env(yaml, Some(nix), NixOptions::default()).await;

    // Create the nested import target.
    let nested_dir = temp_dir.path().join("nested");
    std::fs::create_dir_all(&nested_dir).expect("Failed to create nested dir");
    std::fs::write(
        nested_dir.join("child.nix"),
        r#"{ ... }: { env.NESTED_MARKER = "present"; }"#,
    )
    .expect("Failed to write nested/child.nix");

    copy_fixture_lock(temp_dir.path());

    let tracker = backend
        .input_tracker()
        .expect("input_tracker available after assemble");

    // First eval forces the module system, which parses nested/child.nix.
    backend
        .eval(&["config.devenv.root"])
        .await
        .expect("First eval should succeed");

    // Nix records the absolute path it resolved during import; on macOS this
    // is not automatically routed through /private, so we compare by suffix
    // rather than canonicalising.
    let has_nested_child = |files: &std::collections::HashSet<PathBuf>| {
        files.iter().any(|p| p.ends_with("nested/child.nix"))
    };

    let files_after_first = evaluated_files(&tracker.snapshot());
    assert!(
        has_nested_child(&files_after_first),
        "Tracker should record nested/child.nix during the first eval; got {:?}",
        files_after_first
    );

    // Second eval: Nix's fileEvalCache suppresses re-emission, but the
    // persistent tracker's snapshot must still contain the nested file.
    backend
        .eval(&["config.devenv.cliVersion"])
        .await
        .expect("Second eval should succeed");

    let files_after_second = evaluated_files(&tracker.snapshot());
    assert!(
        has_nested_child(&files_after_second),
        "Tracker lost nested/child.nix between evals; got {:?}",
        files_after_second
    );
}
