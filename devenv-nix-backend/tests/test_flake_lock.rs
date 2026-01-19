//! Integration tests for flake locking functionality

use devenv_core::{CliOptionsConfig, Config, DevenvPaths, GlobalOptions, NixArgs, NixBackend};
use devenv_nix_backend::{ProjectRoot, load_lock_file, nix_backend::NixRustBackend};
use devenv_nix_backend_macros::nix_test;
use nix_bindings_fetchers::FetchersSettings;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio_shutdown::Shutdown;

// Import shared test utilities
mod common;
use common::create_test_cachix_manager;
use common::get_current_system;

/// Helper struct to keep NixArgs and its owned values alive together
struct TestNixArgs {
    tmpdir: PathBuf,
    runtime: PathBuf,
    dotfile_path: PathBuf,
}

impl TestNixArgs {
    fn new(paths: &DevenvPaths) -> Self {
        TestNixArgs {
            tmpdir: PathBuf::from("/tmp"),
            runtime: PathBuf::from("/tmp/runtime"),
            dotfile_path: PathBuf::from(".devenv"),
        }
    }

    fn to_nix_args<'a>(
        &'a self,
        paths: &'a DevenvPaths,
        config: &'a Config,
        nixpkgs_config: devenv_core::config::NixpkgsConfig,
    ) -> NixArgs<'a> {
        NixArgs {
            version: "1.0.0",
            system: get_current_system(),
            devenv_root: &paths.root,
            skip_local_src: false,
            devenv_dotfile: &paths.dotfile,
            devenv_dotfile_path: &self.dotfile_path,
            devenv_tmpdir: &self.tmpdir,
            devenv_runtime: &self.runtime,
            devenv_istesting: true,
            devenv_direnvrc_latest_version: 5,
            container_name: None,
            active_profiles: &[],
            cli_options: CliOptionsConfig::default(),
            hostname: None,
            username: None,
            git_root: None,
            secretspec: None,
            devenv_config: config,
            nixpkgs_config,
        }
    }
}

/// Create a test devenv.yaml file
fn create_test_devenv_yaml(dir: &Path) -> PathBuf {
    let yaml_path = dir.join("devenv.yaml");
    let yaml_content = r#"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-unstable
    flake: true

  devenv:
    url: github:cachix/devenv
    flake: true
"#;
    fs::write(&yaml_path, yaml_content).expect("Failed to write test devenv.yaml");
    yaml_path
}

/// Create a minimal test devenv.yaml with single input
fn create_minimal_devenv_yaml(dir: &Path) -> PathBuf {
    let yaml_path = dir.join("devenv.yaml");
    let yaml_content = r#"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-25.05
    flake: true
"#;
    fs::write(&yaml_path, yaml_content).expect("Failed to write minimal devenv.yaml");
    yaml_path
}

/// Copy fixture lock file to destination directory
/// This avoids unnecessary update() calls in tests that don't specifically test locking
fn copy_fixture_lock(dest_dir: &Path) {
    let fixture_lock = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests/fixtures/devenv.lock");
    let dest_lock = dest_dir.join("devenv.lock");
    fs::copy(&fixture_lock, &dest_lock).expect("Failed to copy fixture lock file");
}

#[test]
fn test_base_dir_loading() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    create_test_devenv_yaml(temp_dir.path());

    // Load config from the temp directory
    let config = Config::load_from(temp_dir.path()).expect("Failed to load config");
    let base_dir = temp_dir.path().to_path_buf();

    // Verify we got the inputs
    assert!(!config.inputs.is_empty(), "Config should have inputs");
    assert!(
        base_dir.is_absolute() || base_dir.canonicalize().is_ok(),
        "Base dir should be valid"
    );
}

#[nix_test]
async fn test_create_flake_inputs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    create_test_devenv_yaml(temp_dir.path());

    // Create minimal devenv.nix - required when assemble() evaluates default.nix
    fs::write(temp_dir.path().join("devenv.nix"), "{ }").expect("Failed to write devenv.nix");

    let config = Config::load_from(temp_dir.path()).expect("Failed to load config");
    let paths = DevenvPaths {
        root: temp_dir.path().to_path_buf(),
        dotfile: temp_dir.path().join(".devenv"),
        dot_gc: temp_dir.path().join(".devenv/gc"),
        home_gc: temp_dir.path().join(".devenv/home-gc"),
    };

    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    // Use offline mode to skip cachix config evaluation in assemble()
    // This test focuses on lock file creation, not cachix configuration
    let global_options = GlobalOptions {
        offline: true,
        ..GlobalOptions::default()
    };
    let backend = NixRustBackend::new(
        paths.clone(),
        config.clone(),
        global_options,
        cachix_manager,
        Shutdown::new(),
        None,
        None,
    )
    .expect("Failed to create backend");

    let test_args = TestNixArgs::new(&paths);
    backend
        .assemble(&test_args.to_nix_args(
            &paths,
            &config,
            config.nixpkgs_config(get_current_system()),
        ))
        .await
        .expect("Failed to assemble backend");

    // Call update() which enables flakes and creates flake inputs
    let result = backend.update(&None).await;

    assert!(
        result.is_ok(),
        "Failed to update/create flake inputs: {:?}",
        result.err()
    );

    // Verify lock file was created
    let lock_file = temp_dir.path().join("devenv.lock");
    assert!(lock_file.exists(), "Lock file should be created");
}

#[test]
fn test_load_nonexistent_lock_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let fetch_settings = FetchersSettings::new().expect("Failed to create fetchers settings");

    let nonexistent = temp_dir.path().join("does-not-exist.lock");
    let result = load_lock_file(&fetch_settings, &nonexistent);

    assert!(result.is_ok(), "Should succeed for nonexistent file");
    assert!(
        result.unwrap().is_none(),
        "Should return None for nonexistent file"
    );
}

#[nix_test]
async fn test_selective_input_update() {
    // Test updating only a specific input while keeping others locked
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    create_test_devenv_yaml(temp_dir.path());

    // Create DevenvPaths for the temp directory
    let paths = DevenvPaths {
        root: temp_dir.path().to_path_buf(),
        dotfile: temp_dir.path().join(".devenv"),
        dot_gc: temp_dir.path().join(".devenv/gc"),
        home_gc: temp_dir.path().join(".devenv/home-gc"),
    };

    // Create directories
    fs::create_dir_all(&paths.dot_gc).expect("Failed to create .devenv/gc");
    fs::create_dir_all(&paths.home_gc).expect("Failed to create home_gc");

    // Load config from devenv.yaml
    let config = Config::load_from(temp_dir.path()).expect("Failed to load config");

    // Create NixBackend
    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    let backend = NixRustBackend::new(
        paths,
        config,
        GlobalOptions::default(),
        cachix_manager,
        Shutdown::new(),
        None,
        None,
    )
    .expect("Failed to create backend");

    // First, create an initial lock (update all inputs)
    backend
        .update(&None)
        .await
        .expect("Failed to create initial lock");

    let lock_path = temp_dir.path().join("devenv.lock");
    assert!(lock_path.exists(), "Initial lock file should be created");

    // Read initial lock to verify nixpkgs was locked
    let initial_lock_content = fs::read_to_string(&lock_path).expect("Failed to read initial lock");
    assert!(
        initial_lock_content.contains("nixpkgs"),
        "Lock should contain nixpkgs"
    );
    assert!(
        initial_lock_content.contains("devenv"),
        "Lock should contain devenv"
    );

    // Now update only nixpkgs
    backend
        .update(&Some("nixpkgs".to_string()))
        .await
        .expect("Failed to update nixpkgs");

    // Verify lock file still exists and was updated
    assert!(lock_path.exists(), "Updated lock file should exist");
    let updated_lock_content = fs::read_to_string(&lock_path).expect("Failed to read updated lock");
    assert!(
        updated_lock_content.contains("nixpkgs"),
        "Updated lock should still contain nixpkgs"
    );
    assert!(
        updated_lock_content.contains("devenv"),
        "Updated lock should still contain devenv"
    );
}

/// Test that demonstrates the full workflow using NixBackend
#[nix_test]
async fn test_full_workflow() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // 1. Create devenv.yaml
    create_minimal_devenv_yaml(temp_dir.path());

    // 2. Create DevenvPaths for the temp directory
    let paths = DevenvPaths {
        root: temp_dir.path().to_path_buf(),
        dotfile: temp_dir.path().join(".devenv"),
        dot_gc: temp_dir.path().join(".devenv/gc"),
        home_gc: temp_dir.path().join(".devenv/home-gc"),
    };

    // Create required directories
    fs::create_dir_all(&paths.dot_gc).expect("Failed to create .devenv/gc");
    fs::create_dir_all(&paths.home_gc).expect("Failed to create home_gc");

    // Load config from devenv.yaml
    let config = Config::load_from(temp_dir.path()).expect("Failed to load config");

    // 3. Create NixBackend
    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    let backend = NixRustBackend::new(
        paths,
        config,
        GlobalOptions::default(),
        cachix_manager,
        Shutdown::new(),
        None,
        None,
    )
    .expect("Failed to create backend");

    // 4. Update all inputs (creates lock file)
    backend
        .update(&None)
        .await
        .expect("Failed to update inputs");

    // 5. Verify the lock file exists and can be read
    let lock_file_path = temp_dir.path().join("devenv.lock");
    assert!(lock_file_path.exists(), "Lock file should exist");

    let content = fs::read_to_string(&lock_file_path).expect("Failed to read lock file");
    assert!(!content.is_empty(), "Lock file should not be empty");
    assert!(
        content.contains("\"nodes\""),
        "Lock file should contain nodes"
    );
    assert!(
        content.contains("nixpkgs"),
        "Lock file should contain nixpkgs input"
    );

    println!(
        "✅ Successfully created lock file at: {}",
        lock_file_path.display()
    );
}

/// Test that relative paths with `..` in the path portion resolve correctly
/// This tests the bug where `path:..?dir=src/modules` was resolving incorrectly
/// because `create_flake_inputs` didn't set base_directory on parse_flags.
#[nix_test]
async fn test_relative_path_with_parent_dir_in_path() {
    // Create structure:
    // temp_dir/
    //   outer/
    //     flake.nix (simple flake we reference)
    //     flake.lock
    //   inner/
    //     devenv.yaml (references path:../outer)
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // Canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let temp_path = temp_dir
        .path()
        .canonicalize()
        .expect("Failed to canonicalize temp dir");
    let outer_dir = temp_path.join("outer");
    let inner_dir = temp_path.join("inner");

    fs::create_dir_all(&outer_dir).expect("Failed to create outer dir");
    fs::create_dir_all(&inner_dir).expect("Failed to create inner dir");

    // Create a minimal flake.nix in outer/
    let flake_nix = r#"{
  inputs = { };
  outputs = { self }: {
    # Minimal flake with no outputs
  };
}"#;
    fs::write(outer_dir.join("flake.nix"), flake_nix).expect("Failed to write flake.nix");

    // Create a minimal flake.lock in outer/
    let flake_lock = r#"{
  "nodes": {
    "root": {}
  },
  "root": "root",
  "version": 7
}"#;
    fs::write(outer_dir.join("flake.lock"), flake_lock).expect("Failed to write flake.lock");

    // Create devenv.yaml in inner/ with relative path using .. in path portion
    let yaml_content = r#"inputs:
  test-outer:
    url: path:..?dir=outer
"#;
    fs::write(inner_dir.join("devenv.yaml"), yaml_content).expect("Failed to write devenv.yaml");

    // Create DevenvPaths for inner directory
    let paths = DevenvPaths {
        root: inner_dir.clone(),
        dotfile: inner_dir.join(".devenv"),
        dot_gc: inner_dir.join(".devenv/gc"),
        home_gc: inner_dir.join(".devenv/home-gc"),
    };

    // Create required directories
    fs::create_dir_all(&paths.dot_gc).expect("Failed to create .devenv/gc");
    fs::create_dir_all(&paths.home_gc).expect("Failed to create home_gc");

    // Load config from devenv.yaml
    let config = Config::load_from(&inner_dir).expect("Failed to load config");

    // Create NixBackend
    let cachix_manager = create_test_cachix_manager(temp_dir.path(), None);
    let backend = NixRustBackend::new(
        paths,
        config,
        GlobalOptions::default(),
        cachix_manager,
        Shutdown::new(),
        None,
        None,
    )
    .expect("Failed to create backend");

    // Update should resolve the relative path correctly
    // Before the fix, this would fail because `..` in the path portion was resolved
    // relative to the wrong base directory
    let result = backend.update(&None).await;

    assert!(
        result.is_ok(),
        "Failed to update with relative path using .. in path portion: {:?}",
        result.err()
    );

    // Verify lock file was created
    let lock_file = inner_dir.join("devenv.lock");
    assert!(lock_file.exists(), "Lock file should be created");

    // Verify the lock file contains our input
    let lock_content = fs::read_to_string(&lock_file).expect("Failed to read lock file");
    assert!(
        lock_content.contains("test-outer"),
        "Lock file should contain test-outer input"
    );
}
