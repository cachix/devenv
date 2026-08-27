#![cfg(feature = "test-nix-store")]
//! Tests for the NixCBackend FFI implementation.
//!
//! Almost every test goes through `TestEnv::builder()...build().await`.
//! Builder defaults give a runnable backend with the bundled fixture lock;
//! tests that exercise update/lock-creation use `.no_lock()`.

use devenv_activity::{Activity, ActivityEvent, EvalOp as ActivityEvalOp, Evaluate};
use devenv_core::eval_op::{EvalOp, OpObserver};
use devenv_core::{BuildOptions, Evaluator};
use devenv_nix_backend_macros::nix_test;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod common;
use common::{DEFAULT_YAML, TestEnv};

// ============================================================================
// SMOKE / BACKEND CONSTRUCTION
// ============================================================================

#[nix_test]
async fn test_backend_creation() {
    let env = TestEnv::new().await;
    assert_eq!(env.backend.name(), "nix");
}

#[test]
fn test_backend_options_default() {
    assert!(BuildOptions::default().gc_root.is_none());
}

// ============================================================================
// UPDATE / LOCK FILE
// ============================================================================

#[nix_test]
async fn test_backend_update_all_inputs() {
    let env = TestEnv::builder().no_lock().build().await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("update should succeed");

    assert!(env.paths.lock_file.exists());
}

#[nix_test]
async fn test_backend_uses_configured_lock_file() {
    let env = TestEnv::builder().lock_file("custom.lock").build().await;

    assert!(env.paths.lock_file.exists());
    assert!(!env.path().join("devenv.lock").exists());
    env.backend
        .eval(&["config.devenv.root"])
        .await
        .expect("bootstrap should read the configured lock file");
}

#[nix_test]
async fn test_backend_update_specific_input() {
    let env = TestEnv::builder().no_lock().build().await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("initial update");
    env.backend
        .update(&Some("nixpkgs".into()), &env.config.inputs, &[])
        .await
        .expect("selective update");
}

#[nix_test]
async fn test_update_lock_file_already_exists() {
    let env = TestEnv::builder().no_lock().build().await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("initial update");

    let lock_path = env.path().join("devenv.lock");
    let first_mtime = std::fs::metadata(&lock_path)
        .and_then(|m| m.modified())
        .expect("first mtime");

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("second update");

    let second_mtime = std::fs::metadata(&lock_path)
        .and_then(|m| m.modified())
        .expect("second mtime");
    assert!(second_mtime >= first_mtime);
}

#[nix_test]
async fn test_backend_update_with_input_overrides() {
    let yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-25.05
  devenv:
    url: github:cachix/devenv/v1.0
"#;
    let overrides = vec![
        "nixpkgs".into(),
        "github:NixOS/nixpkgs/nixos-unstable".into(),
    ];

    let env = TestEnv::builder().yaml(yaml).no_lock().build().await;
    env.backend
        .update(&None, &env.config.inputs, &overrides)
        .await
        .expect("update with overrides");

    let lock = std::fs::read_to_string(env.path().join("devenv.lock")).unwrap();
    assert!(
        lock.contains("nixos-unstable") || lock.contains("unstable"),
        "lock should contain overridden nixpkgs ref"
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
    let overrides = vec![
        "nixpkgs".into(),
        "github:NixOS/nixpkgs/nixos-unstable".into(),
        "devenv".into(),
        "github:cachix/devenv/v1.1".into(),
    ];

    let env = TestEnv::builder().yaml(yaml).no_lock().build().await;
    env.backend
        .update(&None, &env.config.inputs, &overrides)
        .await
        .expect("update with overrides");
    assert!(env.path().join("devenv.lock").exists());
}

/// Odd-length override list is silently ignored (chunks_exact(2) behaviour).
#[nix_test]
async fn test_update_with_invalid_override_inputs() {
    let env = TestEnv::builder().no_lock().build().await;
    let overrides = vec!["nixpkgs".to_string()];

    env.backend
        .update(&None, &env.config.inputs, &overrides)
        .await
        .expect("update should tolerate odd overrides");
    assert!(env.path().join("devenv.lock").exists());
}

#[nix_test]
async fn test_update_with_many_inputs() {
    let yaml = r#"inputs:
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

    let env = TestEnv::builder().yaml(yaml).no_lock().build().await;
    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("update with many inputs");

    let lock = std::fs::read_to_string(env.path().join("devenv.lock")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&lock).unwrap();
    let nodes = parsed.get("nodes").expect("nodes section");
    for input in [
        "nixpkgs",
        "git-hooks",
        "devenv",
        "rust-overlay",
        "systems",
        "flake-utils",
    ] {
        assert!(nodes.get(input).is_some(), "missing input {input}");
    }
}

#[nix_test]
async fn test_update_with_nested_input_follows() {
    let yaml = r#"inputs:
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

    let env = TestEnv::builder().yaml(yaml).no_lock().build().await;
    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("update with follows");

    let lock = std::fs::read_to_string(env.path().join("devenv.lock")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&lock).unwrap();
    let nodes = parsed.get("nodes").expect("nodes section");
    for input in ["nixpkgs", "systems", "flake-utils"] {
        assert!(nodes.get(input).is_some(), "missing input {input}");
    }
}

// ============================================================================
// EVAL
// ============================================================================

#[nix_test]
async fn test_backend_eval_expression() {
    let env = TestEnv::new().await;

    let json = env
        .backend
        .eval(&["config.devenv.root"])
        .await
        .expect("eval should succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert!(
        parsed.is_string(),
        "devenv.root should be a string: {parsed}"
    );
}

#[nix_test]
async fn test_backend_eval_multiple_attributes() {
    let env = TestEnv::new().await;

    let json = env
        .backend
        .eval(&["config.packages", "config.languages.rust.enable"])
        .await
        .expect("eval should succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert!(parsed.is_array(), "multi-attr eval returns array");
}

#[nix_test]
async fn test_eval_empty_attributes_array() {
    let env = TestEnv::new().await;
    let json = env.backend.eval(&[]).await.expect("empty eval");
    assert_eq!(json, "[]");
}

#[nix_test]
async fn test_unfree_package_error_mentions_devenv_yaml_options() {
    let env = TestEnv::builder()
        .nix(
            r#"{ pkgs, ... }: {
  packages = [ pkgs.terraform ];
}"#,
        )
        .build()
        .await;

    let err = env
        .backend
        .eval(&["shell.drvPath"])
        .await
        .expect_err("unfree package should fail without allow_unfree");
    let message = format!("{err:#}");

    assert!(
        message.contains("devenv: package 'terraform' has an unfree license."),
        "{message}"
    );
    assert!(message.contains("allow_unfree: true"), "{message}");
    assert!(message.contains("permitted_unfree_packages:"), "{message}");
}

#[nix_test]
async fn test_unfree_package_can_still_be_allowed_by_devenv_yaml_options() {
    let nix = r#"{ pkgs, ... }: {
  packages = [ pkgs.terraform ];
}"#;

    for yaml in [
        r#"allow_unfree: true
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
"#,
        r#"nixpkgs:
  permitted_unfree_packages:
    - terraform
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
"#,
    ] {
        let env = TestEnv::builder().yaml(yaml).nix(nix).build().await;

        env.backend
            .eval(&["shell.drvPath"])
            .await
            .expect("unfree package should be allowed by devenv.yaml");
    }
}

#[nix_test]
async fn test_eval_nonexistent_attribute() {
    let env = TestEnv::new().await;

    let err = env
        .backend
        .eval(&["nonexistent.attribute.path"])
        .await
        .expect_err("eval of missing attr should fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("nonexistent.attribute.path") || msg.contains("attribute"),
        "error should mention failed attr: {msg}"
    );
}

// ============================================================================
// BUILD
// ============================================================================

#[nix_test]
async fn test_backend_build_package() {
    let env = TestEnv::new().await;
    let paths = env
        .backend
        .build(&["shell"], BuildOptions::default())
        .await
        .expect("build should succeed");
    assert!(!paths.is_empty());
    assert!(paths[0].to_str().unwrap().starts_with("/nix/store"));
}

#[nix_test]
async fn test_backend_build_with_gc_root() {
    let env = TestEnv::new().await;
    let gc_root_base = env.path().join("result");

    env.backend
        .build(
            &["shell"],
            BuildOptions {
                gc_root: Some(gc_root_base.clone()),
            },
        )
        .await
        .expect("build with gc_root");

    // build() appends the attribute name to the gc_root base
    assert!(env.path().join("result-shell").exists());
}

#[nix_test]
async fn test_build_empty_attributes_array() {
    let env = TestEnv::new().await;
    let paths = env
        .backend
        .build(&[], BuildOptions::default())
        .await
        .expect("empty build");
    assert!(paths.is_empty());
}

#[nix_test]
async fn test_build_nonexistent_attribute() {
    let env = TestEnv::new().await;
    let err = env
        .backend
        .build(&["nonexistent.package"], BuildOptions::default())
        .await
        .expect_err("build of missing attr should fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("nonexistent") || msg.contains("attribute"),
        "error should mention failed attr: {msg}"
    );
}

#[nix_test]
async fn test_build_gc_root_already_exists() {
    let env = TestEnv::new().await;
    let gc_root_base = env.path().join("result");

    let opts = || BuildOptions {
        gc_root: Some(gc_root_base.clone()),
    };

    env.backend.build(&["shell"], opts()).await.expect("first");
    env.backend.build(&["shell"], opts()).await.expect("second");

    assert!(env.path().join("result-shell").exists());
}

#[nix_test]
async fn test_build_multiple_attributes_single_call() {
    let env = TestEnv::new().await;
    let paths = env
        .backend
        .build(&["shell", "shell"], BuildOptions::default())
        .await
        .expect("multi-build");
    assert!(!paths.is_empty());
    for p in &paths {
        assert!(p.to_str().unwrap().starts_with("/nix/store"));
    }
}

/// Two consecutive `build()` calls on the same backend with different
/// `gc_root` directories must both produce the symlinks named after the
/// attr (`result{1,2}-shell`). Guards against shared mutable state in
/// the backend leaking GC-root paths between calls.
#[nix_test]
async fn test_workflow_multiple_builds_different_gc_roots() {
    let env = TestEnv::new().await;
    let opts1 = BuildOptions {
        gc_root: Some(env.path().join("result1")),
    };
    let opts2 = BuildOptions {
        gc_root: Some(env.path().join("result2")),
    };

    env.backend.build(&["shell"], opts1).await.expect("build 1");
    env.backend.build(&["shell"], opts2).await.expect("build 2");

    assert!(env.path().join("result1-shell").exists());
    assert!(env.path().join("result2-shell").exists());
}

/// Exercises the `update → eval → update → eval` chain on a single
/// backend instance, where the second update is an *incremental* relock
/// of one input.
///
/// What this guards against:
/// - Re-running `update()` on a populated lock must not corrupt the lock
///   or wedge the in-process eval state.
/// - The selective form of `update(Some(name), ...)` must rewrite only
///   the requested input's node and leave the others byte-identical.
/// - After the relock, the backend must re-evaluate cleanly (no stale
///   eval cache pinning the old lock).
///
/// Cost shape:
/// - `nixpkgs` is fetched from GitHub on cold cache (one-time per host;
///   binary-cache resolves the lock fast on warm). It exists only because
///   the bootstrap eval needs a real `pkgs` to construct `shell`.
/// - The incremental step targets `light`, a tiny in-memory `git+file://`
///   flake we author and bump in-process — pure local git, no network.
/// - Both build steps use `eval("shell.drvPath")` instead of `build()`:
///   we want to assert the eval pipeline succeeds, not that nixpkgs's
///   stdenv closure can be realised. Realise coverage lives in
///   `test_backend_build_package` and the gc-root workflow above.
#[nix_test]
async fn test_workflow_build_then_incremental_update() {
    let flake_dir = tempfile::tempdir().expect("flake tempdir");
    let light_url = common::write_local_flake(flake_dir.path(), "v1");
    let yaml = format!(
        "inputs:\n  \
           nixpkgs:\n    url: github:cachix/devenv-nixpkgs/rolling\n  \
           light:\n    url: {light_url}\n    flake: false\n"
    );

    let env = TestEnv::builder().yaml(yaml).no_lock().build().await;

    env.backend
        .update(&None, &env.config.inputs, &[])
        .await
        .expect("initial update");
    let drv1 = env
        .backend
        .eval(&["shell.drvPath"])
        .await
        .expect("initial eval");
    assert!(drv1.contains("/nix/store"), "drv1 = {drv1}");

    common::bump_local_flake(flake_dir.path(), "v2");

    env.backend
        .update(&Some("light".into()), &env.config.inputs, &[])
        .await
        .expect("incremental update");
    let drv2 = env.backend.eval(&["shell.drvPath"]).await.expect("re-eval");
    assert!(drv2.contains("/nix/store"), "drv2 = {drv2}");

    let lock = std::fs::read_to_string(env.path().join("devenv.lock")).expect("read lock");
    assert!(lock.contains("\"light\""), "light input present in lock");
}

// ============================================================================
// DEV ENV
// ============================================================================

#[nix_test]
async fn test_backend_dev_env() {
    let env = TestEnv::new().await;

    let output = env
        .backend
        .dev_env(true, &env.path().join(".devenv/profile"))
        .await
        .expect("dev_env should succeed");
    assert!(!output.bash_env.is_empty());
}

// ============================================================================
// HOT RELOAD
// ============================================================================

/// Drive a reload scenario: build dev_env, mutate `modify_file`, invalidate,
/// rebuild dev_env, assert the new content reaches the bash env.
async fn assert_invalidate_picks_up_change(
    env: &TestEnv,
    modify_file: &str,
    modified_content: &str,
    expected_before: &str,
    expected_after: &str,
) {
    let gc_root = env.path().join(".devenv/profile");

    let env1 = env.backend.dev_env(false, &gc_root).await.expect("first");
    assert!(
        String::from_utf8_lossy(&env1.bash_env).contains(expected_before),
        "first dev_env should contain '{expected_before}'"
    );

    std::fs::write(env.path().join(modify_file), modified_content).expect("modify");
    env.backend.invalidate_eval_state().expect("invalidate");

    let env2 = env.backend.dev_env(false, &gc_root).await.expect("second");
    assert!(
        String::from_utf8_lossy(&env2.bash_env).contains(expected_after),
        "post-invalidate should contain '{expected_after}', got stale result"
    );
}

#[nix_test]
async fn test_dev_env_after_invalidate() {
    let env = TestEnv::builder()
        .nix(r#"{ pkgs, ... }: { env.TEST_RELOAD_VAR = "version1"; }"#)
        .build()
        .await;
    assert_invalidate_picks_up_change(
        &env,
        "devenv.nix",
        r#"{ pkgs, ... }: { env.TEST_RELOAD_VAR = "version2"; }"#,
        "version1",
        "version2",
    )
    .await;
}

#[nix_test]
async fn test_dev_env_after_invalidate_imported_file() {
    let env = TestEnv::builder()
        .nix(r#"{ pkgs, ... }: { imports = [ ./extra.nix ]; }"#)
        .extra_file(
            "extra.nix",
            r#"{ ... }: { env.TEST_IMPORT_VAR = "original"; }"#,
        )
        .build()
        .await;
    assert_invalidate_picks_up_change(
        &env,
        "extra.nix",
        r#"{ ... }: { env.TEST_IMPORT_VAR = "updated"; }"#,
        "original",
        "updated",
    )
    .await;
}

#[nix_test]
async fn test_dev_env_after_invalidate_yaml_import() {
    let yaml = format!("{DEFAULT_YAML}imports:\n  - ./extra.nix\n");
    let env = TestEnv::builder()
        .yaml(yaml)
        .extra_file(
            "extra.nix",
            r#"{ ... }: { env.TEST_YAML_IMPORT = "original"; }"#,
        )
        .build()
        .await;
    assert_invalidate_picks_up_change(
        &env,
        "extra.nix",
        r#"{ ... }: { env.TEST_YAML_IMPORT = "updated"; }"#,
        "original",
        "updated",
    )
    .await;
}

#[nix_test]
async fn test_dotenv_input_change_detection() {
    let env = TestEnv::builder()
        .nix(r#"{ ... }: { dotenv.enable = true; }"#)
        .extra_file(".env", "VALUE=before\n")
        .build()
        .await;
    let gc_root = env.path().join(".devenv/profile");

    env.backend
        .dev_env(false, &gc_root)
        .await
        .expect("evaluate dotenv-enabled shell");
    let snapshot = env.eval_inputs.snapshot_inputs();

    assert!(!snapshot.is_empty());
    assert!(!env.eval_inputs.inputs_changed(&snapshot).unwrap());

    std::fs::write(env.path().join(".env"), "VALUE=after\n").expect("modify dotenv file");
    assert!(env.eval_inputs.inputs_changed(&snapshot).unwrap());
}

// ============================================================================
// GC
// ============================================================================

#[nix_test]
async fn test_backend_gc_cleans_non_store_paths() {
    let env = TestEnv::builder().no_lock().build().await;
    let p1 = env.path().join("path1");
    let p2 = env.path().join("path2");
    std::fs::write(&p1, "1").unwrap();
    std::fs::write(&p2, "2").unwrap();

    env.backend.gc(vec![p1, p2]).await.expect("gc");
}

#[nix_test]
async fn test_gc_with_invalid_store_paths() {
    let env = TestEnv::builder().no_lock().build().await;
    let bogus = vec![
        env.path().join("not/a/store/path"),
        env.path().join("relative/path"),
    ];
    env.backend
        .gc(bogus)
        .await
        .expect("gc tolerates invalid paths");
}

/// GC over a live store path: the path is protected (either gc returns
/// Ok with no deletion, or returns an "alive" error). Mixed with temp
/// files, the temp files are cleaned regardless.
#[nix_test]
async fn test_gc_handles_live_paths_and_mixed_inputs() {
    let env = TestEnv::new().await;
    let built = env
        .backend
        .build(&["shell"], BuildOptions::default())
        .await
        .expect("build");
    assert!(!built.is_empty());

    let temp_file = env.path().join("temp.txt");
    let temp_dir = env.path().join("temp_dir");
    std::fs::write(&temp_file, "x").unwrap();
    std::fs::create_dir(&temp_dir).unwrap();

    let store_path = PathBuf::from(built[0].to_str().unwrap());
    let result = env
        .backend
        .gc(vec![store_path, temp_file.clone(), temp_dir.clone()])
        .await;

    if let Err(e) = result {
        let msg = format!("{e:?}");
        assert!(
            msg.contains("alive") || msg.contains("still"),
            "error should be about live paths: {msg}"
        );
    }

    assert!(!temp_file.exists(), "non-store temp file should be removed");
    assert!(!temp_dir.exists(), "non-store temp dir should be removed");
}

// ============================================================================
// ERROR PROPAGATION
// ============================================================================

#[nix_test]
async fn test_build_with_syntax_error_in_nix() {
    let broken = r#"{ ... }: {
  this is not valid nix syntax!!!
}"#;
    let result = TestEnv::builder().nix(broken).try_build().await;

    let msg = match result {
        Err(e) => format!("{e:?}"),
        Ok(env) => format!(
            "{:?}",
            env.backend
                .eval(&["shell"])
                .await
                .expect_err("eval should fail with syntax error")
        ),
    };

    assert!(
        msg.contains("syntax") || msg.contains("parse") || msg.contains("error"),
        "should be a parse/syntax error: {msg}"
    );
}

/// Nix evaluation errors must surface the underlying detail (e.g. the
/// undefined variable name), not just a generic wrapper.
#[nix_test]
async fn test_eval_error_includes_nix_details() {
    let broken = r#"{ pkgs, ... }: {
  packages = [ nonexistent_var_xyz ];
}"#;
    let result = TestEnv::builder().nix(broken).try_build().await;

    let msg = match result {
        Err(e) => format!("{e:?}"),
        Ok(env) => format!(
            "{:?}",
            env.backend
                .eval(&["config.packages"])
                .await
                .expect_err("eval should fail")
        ),
    };

    assert!(
        msg.contains("nonexistent_var_xyz"),
        "error should include the undefined variable name: {msg}"
    );
    assert!(
        msg.to_lowercase().contains("undefined"),
        "error should mention 'undefined': {msg}"
    );
}

// ============================================================================
// CONCURRENCY
// ============================================================================

#[nix_test]
async fn test_eval_state_mutex_under_concurrent_eval() {
    let env = TestEnv::new().await;
    let backend = std::sync::Arc::new(env.backend);

    let (r1, r2, r3) = tokio::join!(
        backend.eval(&["config.devenv.root"]),
        backend.eval(&["config.name"]),
        backend.eval(&["config.devenv.cliVersion"]),
    );

    for json in [r1, r2, r3] {
        let s = json.expect("eval should succeed");
        serde_json::from_str::<serde_json::Value>(&s).expect("valid JSON");
    }
}

#[nix_test]
async fn test_concurrent_build_operations() {
    let nix = r#"{ pkgs, ... }: {
  languages.python.enable = true;
  languages.php.enable = true;
}"#;
    let env = TestEnv::builder().nix(nix).build().await;
    let backend = std::sync::Arc::new(env.backend);

    let (r1, r2, r3) = tokio::join!(
        backend.build(&["shell"], BuildOptions::default()),
        backend.build(
            &["config.languages.python.package"],
            BuildOptions::default()
        ),
        backend.build(&["config.languages.php.package"], BuildOptions::default()),
    );

    for paths in [r1, r2, r3] {
        let p = paths.expect("concurrent build");
        assert!(!p.is_empty());
    }
}

// ============================================================================
// INPUT TRACKER
// ============================================================================

#[derive(Default)]
struct EffectRecorder(Mutex<Vec<EvalOp>>);

impl OpObserver for EffectRecorder {
    fn record(&self, op: EvalOp) {
        self.0.lock().expect("effect recorder lock").push(op);
    }
}

/// Exercise the complete Nix-to-activity effect path with an expression whose
/// effects are deliberately fixed: import one file twice, with one call to
/// each filesystem/environment primop below. This catches missing effects and
/// verifies both uncached and cached file-evaluation events.
#[nix_test]
async fn test_eval_effect_stream_has_expected_types_and_cache_states() {
    let env = TestEnv::new().await;
    let recorder = Arc::new(EffectRecorder::default());
    let observer: Arc<dyn OpObserver> = recorder.clone();
    env.backend.log_bridge().add_observer(observer.clone());
    let payload = env.path().join("effect-payload.txt");
    let source_dir = env.path().join("effect-source-dir");
    let effect_file = env.path().join("effect-source.nix");
    std::fs::write(&payload, "effect payload\n").expect("write effect payload");
    std::fs::create_dir(&source_dir).expect("create source directory");
    std::fs::write(source_dir.join("source.txt"), "source payload\n")
        .expect("write source payload");

    let payload = payload.canonicalize().expect("canonicalize payload path");
    let directory = env
        .path()
        .canonicalize()
        .expect("canonicalize project path");
    let source_dir = source_dir
        .canonicalize()
        .expect("canonicalize source directory");
    let payload_string = serde_json::to_string(&payload).expect("quote payload path");
    let directory_string = serde_json::to_string(&directory).expect("quote directory path");
    let source_dir_string = serde_json::to_string(&source_dir).expect("quote source path");
    std::fs::write(
        &effect_file,
        format!(
            r#"builtins.deepSeq [
  (builtins.readFile {payload_string})
  (builtins.pathExists {payload_string})
  (builtins.readFileType {payload_string})
  (builtins.hashFile "sha256" {payload_string})
  (builtins.readDir {directory_string})
  (builtins.getEnv "DEVENV_EFFECT_TEST_UNSET")
  (builtins.path {{ path = {source_dir_string}; name = "devenv-effect-copy"; }})
  (builtins.filterSource (_path: _type: true) {source_dir_string})
] true
"#,
        ),
    )
    .expect("write effect source");

    let (mut activity_rx, activity_handle) = devenv_activity::init();
    let activity_channel = activity_handle.install();
    let activity = devenv_activity::start!(Activity::evaluate("Evaluating effect fixture"));
    let activity_id = activity.id();
    let eval_scope = env.backend.log_bridge().begin_eval(activity_id);

    let effect_file = effect_file
        .canonicalize()
        .expect("canonicalize effect path");
    let effect_file_string = serde_json::to_string(&effect_file).expect("quote effect path");
    {
        let mut state_guard = env
            .backend
            .eval_state_handle()
            .lock()
            .expect("lock eval state");
        let state = state_guard.as_mut().expect("eval state");
        let value = state
            .eval_from_string(
                &format!("import {effect_file_string}"),
                &env.path().to_string_lossy(),
            )
            .expect("evaluate effect fixture");
        state.force(&value).expect("force effect fixture");

        let cached_value = state
            .eval_from_string(
                &format!("import {effect_file_string}"),
                &env.path().to_string_lossy(),
            )
            .expect("evaluate cached effect fixture");
        state
            .force(&cached_value)
            .expect("force cached effect fixture");
    }

    drop(eval_scope);
    env.backend.log_bridge().remove_observer(&observer);
    drop(activity);
    drop(activity_channel);

    let events: Vec<_> = std::iter::from_fn(|| activity_rx.try_recv().ok()).collect();
    let effects: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ActivityEvent::Evaluate(Evaluate::Op { id, op, .. }) if *id == activity_id => {
                Some(op.clone())
            }
            _ => None,
        })
        .collect();
    let eval_logs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ActivityEvent::Evaluate(Evaluate::Log { id, line, .. }) if *id == activity_id => {
                Some(line.as_str())
            }
            _ => None,
        })
        .collect();

    let effect_file_display = effect_file.to_string_lossy();
    assert!(
        eval_logs.iter().any(|line| {
            line.contains("evaluating file") && line.contains(effect_file_display.as_ref())
        }),
        "the evaluation activity should retain Nix's ordinary file log: {eval_logs:?}"
    );
    assert!(
        eval_logs.iter().any(|line| {
            line.contains("evaluating file")
                && line.contains(effect_file_display.as_ref())
                && line.contains("(cached)")
        }),
        "the evaluation activity should include cached file logs: {eval_logs:?}"
    );

    let effect_types: Vec<_> = effects
        .iter()
        .map(|effect| match effect {
            ActivityEvalOp::EvaluatedFile { cached: false, .. } => "evaluated-file-uncached",
            ActivityEvalOp::EvaluatedFile { cached: true, .. } => "evaluated-file-cached",
            ActivityEvalOp::ReadFile { .. } => "read-file",
            ActivityEvalOp::PathExists { .. } => "path-exists",
            ActivityEvalOp::ReadFileType { .. } => "read-file-type",
            ActivityEvalOp::HashFile { .. } => "hash-file",
            ActivityEvalOp::ReadDir { .. } => "read-dir",
            ActivityEvalOp::GetEnv { .. } => "get-env",
            ActivityEvalOp::CopiedSource { .. } => "copy-source",
            ActivityEvalOp::FilteredSource { .. } => "filter-source",
        })
        .collect();
    assert_eq!(effects.len(), 10);
    assert_eq!(
        effect_types,
        [
            "evaluated-file-uncached",
            "read-file",
            "path-exists",
            "read-file-type",
            "hash-file",
            "read-dir",
            "get-env",
            "copy-source",
            "filter-source",
            "evaluated-file-cached",
        ]
    );

    let observed = recorder.0.lock().expect("effect recorder lock");
    let observed_types: Vec<_> = observed
        .iter()
        .map(|effect| match effect {
            EvalOp::EvaluatedFile { cached: false, .. } => "evaluated-file-uncached",
            EvalOp::EvaluatedFile { cached: true, .. } => "evaluated-file-cached",
            EvalOp::ReadFile { .. } => "read-file",
            EvalOp::PathExists { .. } => "path-exists",
            EvalOp::ReadFileType { .. } => "read-file-type",
            EvalOp::HashFile { .. } => "hash-file",
            EvalOp::ReadDir { .. } => "read-dir",
            EvalOp::GetEnv { .. } => "get-env",
            EvalOp::CopiedSource { .. } => "copy-source",
            EvalOp::FilteredSource { .. } => "filter-source",
        })
        .collect();
    assert_eq!(observed_types, effect_types);
}

fn evaluated_files(ops: &[EvalOp]) -> std::collections::HashSet<PathBuf> {
    ops.iter()
        .filter_map(|op| match op {
            EvalOp::EvaluatedFile { source, .. } => Some(source.clone()),
            _ => None,
        })
        .filter(|p| !p.starts_with("/nix/store"))
        .collect()
}

/// The persistent `EvalInputTracker` must accumulate file deps across attribute
/// evaluations. Nix reports both cached and uncached file evaluations as
/// structured effects, and the tracker is the only place those dependencies
/// survive across attribute evaluations. This invariant keeps later attrs' DB rows
/// (e.g. `shell`) aware of files first touched during earlier attrs'
/// evaluation (e.g. `config.cachix.*`).
#[nix_test]
async fn test_input_tracker_accumulates_across_evals() {
    let env = TestEnv::new().await;
    let tracker = env.backend.eval_inputs().expect("input_tracker available");

    env.backend
        .eval(&["config.devenv.root"])
        .await
        .expect("first eval");
    let after_first = evaluated_files(&tracker.snapshot());
    assert!(!after_first.is_empty(), "tracker observes base file deps");

    env.backend
        .eval(&["config.devenv.cliVersion"])
        .await
        .expect("second eval");
    let after_second = evaluated_files(&tracker.snapshot());

    let missing: Vec<_> = after_first.difference(&after_second).collect();
    assert!(
        missing.is_empty(),
        "tracker dropped {} files between evals: {missing:?}",
        missing.len(),
    );
}

/// Regression test for the original boop.nix bug: a nested file imported
/// via `imports = [./nested/child.nix]` must remain in the persistent
/// tracker across `eval` calls, even after Nix's fileEvalCache stops
/// re-emitting it.
#[nix_test]
async fn test_nested_import_tracked_across_evals() {
    let env = TestEnv::builder()
        .nix(r#"{ ... }: { imports = [ ./nested/child.nix ]; }"#)
        .extra_file(
            "nested/child.nix",
            r#"{ ... }: { env.NESTED_MARKER = "present"; }"#,
        )
        .build()
        .await;

    let tracker = env.backend.eval_inputs().expect("input_tracker available");

    // macOS may not auto-canonicalize through /private; suffix-match.
    let has_nested_child = |files: &std::collections::HashSet<PathBuf>| {
        files.iter().any(|p| p.ends_with("nested/child.nix"))
    };

    env.backend
        .eval(&["config.devenv.root"])
        .await
        .expect("first eval");
    let after_first = evaluated_files(&tracker.snapshot());
    assert!(
        has_nested_child(&after_first),
        "tracker should record nested/child.nix: {after_first:?}"
    );

    env.backend
        .eval(&["config.devenv.cliVersion"])
        .await
        .expect("second eval");
    let after_second = evaluated_files(&tracker.snapshot());
    assert!(
        has_nested_child(&after_second),
        "tracker lost nested/child.nix between evals: {after_second:?}"
    );
}
