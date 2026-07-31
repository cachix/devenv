#![cfg(feature = "test-nix-store")]
//! Integration tests for flake locking.

use devenv_core::{Config, NixOptions, NixSettings};
use devenv_nix_backend::{load_lock_file, lock};
use devenv_nix_backend_macros::nix_test;
use nix_bindings_fetchers::FetchersSettings;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::TempDir;

mod common;
use common::TestEnv;

const MULTI_INPUT_YAML: &str = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-unstable
    flake: true
  devenv:
    url: github:cachix/devenv
    flake: true
"#;

const MINIMAL_YAML: &str = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-25.05
    flake: true
"#;

#[test]
fn test_base_dir_loading() {
    let temp_dir = TempDir::new().expect("temp dir");
    fs::write(temp_dir.path().join("devenv.yaml"), MULTI_INPUT_YAML).unwrap();

    let config = Config::load_from(temp_dir.path()).expect("load config");
    assert!(!config.inputs.is_empty(), "config should have inputs");
}

#[test]
fn test_load_nonexistent_lock_file() {
    let temp_dir = TempDir::new().expect("temp dir");
    let fetch = FetchersSettings::new().expect("fetchers");
    let result = load_lock_file(&fetch, &temp_dir.path().join("missing.lock"));
    assert!(result.unwrap().is_none(), "missing file should map to None");
}

#[nix_test]
async fn test_create_flake_inputs() {
    let env = TestEnv::builder()
        .yaml(MULTI_INPUT_YAML)
        // offline mode skips cachix bootstrap eval — this test is about lock creation
        .nix_options(NixOptions {
            offline: Some(true),
            ..Default::default()
        })
        .no_lock()
        .build()
        .await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("update should create lock");
    assert!(env.path().join("devenv.lock").exists());
}

#[nix_test]
async fn test_selective_input_update() {
    let env = TestEnv::builder()
        .yaml(MULTI_INPUT_YAML)
        .no_lock()
        .build()
        .await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("initial lock");

    let lock_path = env.path().join("devenv.lock");
    let initial = fs::read_to_string(&lock_path).expect("read lock");
    assert!(initial.contains("nixpkgs") && initial.contains("devenv"));

    env.backend
        .update(&Some("nixpkgs".into()), &env.config.inputs, &[])
        .await
        .expect("selective update");

    let updated = fs::read_to_string(&lock_path).expect("read lock");
    assert!(updated.contains("nixpkgs") && updated.contains("devenv"));
}

#[nix_test]
async fn test_full_workflow() {
    let env = TestEnv::builder()
        .yaml(MINIMAL_YAML)
        .no_lock()
        .build()
        .await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("update");

    let lock = fs::read_to_string(env.path().join("devenv.lock")).expect("read lock");
    assert!(lock.contains("\"nodes\""), "lock should have nodes");
    assert!(lock.contains("nixpkgs"));
}

#[nix_test]
async fn test_locked_input_paths_resolve_follows_and_subdirectory() {
    let remote = TempDir::new().expect("remote temp dir");
    let remote_url = common::write_local_flake(remote.path(), "remote-config");
    let config_dir = remote.path().join("config");
    fs::create_dir(&config_dir).expect("create config dir");
    fs::create_dir(config_dir.join("tool")).expect("create remote tool dir");
    fs::write(
        config_dir.join("devenv.yaml"),
        "inputs:\n  remote-tool:\n    url: path:./tool\n    flake: false\nreload: false\n",
    )
    .expect("write remote yaml");
    fs::write(config_dir.join("devenv.nix"), "{ }\n").expect("write remote module");
    fs::write(config_dir.join("tool/marker"), "remote tool\n").expect("write remote tool");
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=test@devenv",
            "-c",
            "user.name=devenv-test",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(remote.path())
        .args(["add", "config"])
        .status()
        .expect("git add remote config");
    assert!(status.success());
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=test@devenv",
            "-c",
            "user.name=devenv-test",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(remote.path())
        .args(["commit", "-q", "-m", "add devenv config"])
        .status()
        .expect("git commit remote config");
    assert!(status.success());

    let yaml = format!(
        r#"inputs:
  shared:
    url: {remote_url}?dir=config
    flake: false
  shared-alias:
    follows: shared
imports:
  - shared-alias
"#
    );
    let env = TestEnv::builder().yaml(yaml).no_lock().build().await;
    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("lock local remote input");

    let mut eval_state = env.backend.fresh_eval_state().expect("fresh eval state");
    let paths = lock::input_paths(
        &mut eval_state,
        env.path(),
        &BTreeSet::from(["shared-alias".to_string()]),
    )
    .expect("resolve locked input paths");
    let source = &paths["shared-alias"];
    assert!(
        source.starts_with("/nix/store"),
        "source = {}",
        source.display()
    );
    assert_eq!(
        fs::read_to_string(source.join("tool/marker")).expect("read resolved source"),
        "remote tool\n"
    );

    let compose = || {
        let config = Config::load_from(env.path()).expect("load root config");
        lock::resolve_config_imports(
            env.path(),
            config,
            &NixSettings::default(),
            |input_sources| {
                Config::load_from_with_source_and_input_sources(env.path(), None, input_sources)
            },
        )
        .expect("compose remote config")
    };

    let composed = compose();
    assert!(composed.inputs.contains_key("remote-tool"));

    // A second composition starts from root YAML again. Keep the completed
    // lock read-only to prove that the discovery pass does not temporarily
    // prune imported inputs and rewrite the same graph twice.
    let lock_path = env.path().join("devenv.lock");
    let mut permissions = fs::metadata(&lock_path)
        .expect("lock metadata")
        .permissions();
    permissions.set_mode(0o444);
    fs::set_permissions(&lock_path, permissions).expect("make lock read-only");
    let composed_again = compose();
    assert!(composed_again.inputs.contains_key("remote-tool"));
}

/// Regression: relative paths with `..` in the path portion (e.g.
/// `path:..?dir=outer`) used to resolve against the wrong base because
/// `create_flake_inputs` didn't set `base_directory` on parse_flags.
#[nix_test]
async fn test_relative_path_with_parent_dir_in_path() {
    // Layout: temp/outer/{flake.nix,flake.lock}, temp/inner/devenv.yaml
    let temp_dir = TempDir::new().expect("temp dir");
    let temp_path = temp_dir
        .path()
        .canonicalize()
        .expect("canonicalize (resolve macOS /var → /private/var)");
    let outer = temp_path.join("outer");
    let inner = temp_path.join("inner");
    fs::create_dir_all(&outer).unwrap();
    fs::create_dir_all(&inner).unwrap();

    fs::write(
        outer.join("flake.nix"),
        r#"{
  inputs = { };
  outputs = { self }: { };
}"#,
    )
    .unwrap();
    fs::write(
        outer.join("flake.lock"),
        r#"{ "nodes": { "root": {} }, "root": "root", "version": 7 }"#,
    )
    .unwrap();

    let yaml = r#"inputs:
  test-outer:
    url: path:..?dir=outer
"#;
    fs::write(inner.join("devenv.yaml"), yaml).unwrap();

    // Manual setup since the project root is `inner`, not the TempDir root.
    let cwd_guard = common::CwdGuard::enter(&inner);
    fs::write(inner.join("devenv.nix"), common::DEFAULT_NIX).unwrap();
    let paths = common::paths_under(&inner);
    let config = Config::load_from(&inner).expect("load config");
    let backend =
        common::init_backend(paths, config.clone(), NixOptions::default()).expect("init backend");

    backend
        .update(&None, &config.inputs, &[])
        .await
        .expect("update should resolve `..` correctly");

    let lock = fs::read_to_string(inner.join("devenv.lock")).expect("read lock");
    assert!(lock.contains("test-outer"));

    drop(cwd_guard);
}
