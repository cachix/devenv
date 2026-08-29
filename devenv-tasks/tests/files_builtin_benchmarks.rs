//! Opt-in timing probes for the native `files-reconcile` builtin.
//!
//! These are integration tests rather than a benchmark harness on purpose.
//! `timing=cold-end-to-end` includes task construction, a fresh SQLite task
//! database, lock/state setup, and reconciliation. `timing=prebuilt-run`
//! constructs the task before starting the clock, for comparisons where task
//! setup would otherwise obscure the reconciliation cost. Run them explicitly
//! (and never use their timings as a cross-host regression threshold):
//!
//! `cargo test -p devenv-tasks --test files_builtin_benchmarks benchmark_copy_lifecycle_matrix_for_flat_files_and_trees -- --ignored --nocapture`

#![cfg(unix)]

use devenv_tasks::{Config, Tasks};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio_shutdown::Shutdown;

const TASK_NAME: &str = "devenv:benchmark:files";
const FLAT_FILES: usize = 256;
const TREE_WIDTH: usize = 16;
const TREE_DEPTH: usize = 16;
// Each destination is independently replaced, so an update creates 256
// distinct deferred directory removals. This exceeds the per-run cleanup
// record budget while keeping the source fixture modest.
const MANY_DIRECTORIES: usize = 256;
const MANY_DIRECTORY_FILES: usize = 4;
// Large enough that the replacement has a visible recursive cleanup phase,
// without making this opt-in probe unreasonable on a laptop or CI worker.
const LARGE_TREE_WIDTH: usize = 64;
const LARGE_TREE_DEPTH: usize = 128;

fn spec(path: impl AsRef<str>, source: &Path) -> Value {
    json!({ "path": path.as_ref(), "source": source, "mode": "copy" })
}

fn fallback(cache: &Path, tag: &str) -> PathBuf {
    fs::create_dir_all(cache).unwrap();
    let command = cache.join(format!("fallback-{tag}.sh"));
    fs::write(&command, "#!/bin/sh\nexit 97\n").unwrap();
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
    command
}

async fn build_task(
    root: &Path,
    state: &Path,
    cache: &Path,
    tag: &str,
    digest: &str,
    files: Vec<Value>,
    shutdown: std::sync::Arc<Shutdown>,
) -> Tasks {
    let config = Config::try_from(json!({
        "roots": [TASK_NAME],
        "run_mode": "all",
        "tasks": [{
            "name": TASK_NAME,
            "command": fallback(cache, tag),
            "builtin": {
                "name": "files-reconcile",
                "version": 1,
                "input": {
                    "root": root,
                    "state_file": state,
                    "desired_digest": digest,
                    "files": files,
                }
            }
        }]
    }))
    .unwrap();

    Tasks::builder(config, devenv_core::VerbosityLevel::Quiet, shutdown)
        .with_db_path(cache.join(format!("tasks-{tag}.db")))
        .build()
        .await
        .unwrap()
}

async fn run(root: &Path, state: &Path, cache: &Path, tag: &str, digest: &str, files: Vec<Value>) {
    let tasks = build_task(root, state, cache, tag, digest, files, Shutdown::new()).await;
    run_tasks(&tasks, tag).await;
}

async fn run_tasks(tasks: &Tasks, tag: &str) {
    tasks.run(false).await;
    let status = tasks.get_completion_status().await;
    assert_eq!(status.succeeded, 1, "{tag}: builtin did not succeed");
    assert_eq!(status.failed, 0, "{tag}: builtin failed");
    assert_eq!(status.cancelled, 0, "{tag}: builtin was cancelled");
}

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    first: PathBuf,
    second: PathBuf,
}

impl Fixture {
    fn flat() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let sources = temp.path().join("sources");
        fs::create_dir(&sources).unwrap();
        let first = sources.join("flat-v1");
        let second = sources.join("flat-v2");
        fs::write(&first, vec![b'a'; 64 * 1024]).unwrap();
        fs::write(&second, vec![b'b'; 64 * 1024]).unwrap();
        fs::set_permissions(&first, fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(&second, fs::Permissions::from_mode(0o444)).unwrap();
        Self {
            root: temp.path().join("project"),
            state: temp.path().join("state/files.json"),
            cache: temp.path().join("cache"),
            first,
            second,
            _temp: temp,
        }
    }

    fn tree() -> Self {
        Self::tree_with_shape(TREE_WIDTH, TREE_DEPTH)
    }

    fn large_tree() -> Self {
        Self::tree_with_shape(LARGE_TREE_WIDTH, LARGE_TREE_DEPTH)
    }

    fn many_directories() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let sources = temp.path().join("sources");
        fs::create_dir(&sources).unwrap();
        let first = sources.join("directories-v1");
        let second = sources.join("directories-v2");
        for (tree, contents) in [(&first, b'a'), (&second, b'b')] {
            for directory in 0..MANY_DIRECTORIES {
                let directory = tree.join(format!("dir-{directory:04}"));
                fs::create_dir_all(&directory).unwrap();
                for file in 0..MANY_DIRECTORY_FILES {
                    fs::write(
                        directory.join(format!("file-{file:02}")),
                        vec![contents; 4096],
                    )
                    .unwrap();
                }
            }
        }
        Self {
            root: temp.path().join("project"),
            state: temp.path().join("state/files.json"),
            cache: temp.path().join("cache"),
            first,
            second,
            _temp: temp,
        }
    }

    fn tree_with_shape(width: usize, depth: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let sources = temp.path().join("sources");
        fs::create_dir(&sources).unwrap();
        let first = sources.join("tree-v1");
        let second = sources.join("tree-v2");
        for tree in [&first, &second] {
            for directory in 0..width {
                let directory = tree.join(format!("dir-{directory:02}"));
                fs::create_dir_all(&directory).unwrap();
                for file in 0..depth {
                    fs::write(directory.join(format!("file-{file:02}")), vec![b'a'; 4096]).unwrap();
                }
            }
        }
        // Change one leaf as well as the root source identity for update-path
        // coverage; every copied tree must remain writable after materializing.
        fs::write(second.join("dir-00/file-00"), vec![b'b'; 4096]).unwrap();
        fs::set_permissions(
            first.join("dir-00/file-00"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(
            second.join("dir-00/file-00"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        Self {
            root: temp.path().join("project"),
            state: temp.path().join("state/files.json"),
            cache: temp.path().join("cache"),
            first,
            second,
            _temp: temp,
        }
    }
}

fn flat_specs(source: &Path) -> Vec<Value> {
    (0..FLAT_FILES)
        .map(|number| spec(format!("flat/file-{number:04}"), source))
        .collect()
}

fn tree_specs(source: &Path) -> Vec<Value> {
    vec![spec("tree", source)]
}

fn many_directory_specs(source_root: &Path) -> Vec<Value> {
    (0..MANY_DIRECTORIES)
        .map(|directory| {
            let name = format!("dir-{directory:04}");
            spec(format!("many/{name}"), &source_root.join(name))
        })
        .collect()
}

fn symlink_specs(source: &Path) -> Vec<Value> {
    (0..FLAT_FILES)
        .map(|number| {
            json!({
                "path": format!("links/file-{number:04}"),
                "source": source,
                "mode": "symlink",
            })
        })
        .collect()
}

async fn time_cold_end_to_end(
    fixture: &Fixture,
    shape: &str,
    operation: &str,
    digest: &str,
    files: Vec<Value>,
) {
    let started = Instant::now();
    run(
        &fixture.root,
        &fixture.state,
        &fixture.cache,
        &format!("{shape}-{operation}"),
        digest,
        files,
    )
    .await;
    eprintln!(
        "files-reconcile benchmark timing=cold-end-to-end shape={shape} operation={operation} elapsed_us={}",
        started.elapsed().as_micros()
    );
}

async fn time_prebuilt_run(tasks: &Tasks, shape: &str, operation: &str, tag: &str) -> Duration {
    let started = Instant::now();
    run_tasks(tasks, tag).await;
    let elapsed = started.elapsed();
    eprintln!(
        "files-reconcile benchmark timing=prebuilt-run shape={shape} operation={operation} elapsed_us={}",
        elapsed.as_micros()
    );
    elapsed
}

async fn lifecycle_matrix(
    fixture: &Fixture,
    shape: &str,
    update_operation: &str,
    specs: impl Fn(&Path) -> Vec<Value>,
    sample: &Path,
    updated_sample: &Path,
    validate_materialization: impl Fn(&Path),
) {
    time_cold_end_to_end(fixture, shape, "create", "v1", specs(&fixture.first)).await;
    assert_eq!(
        fs::metadata(sample).unwrap().permissions().mode() & 0o200,
        0o200
    );
    validate_materialization(&fixture.first);

    time_cold_end_to_end(fixture, shape, "unchanged", "v1", specs(&fixture.first)).await;
    time_cold_end_to_end(
        fixture,
        shape,
        update_operation,
        "v2",
        specs(&fixture.second),
    )
    .await;
    assert_eq!(
        fs::read(sample).unwrap(),
        fs::read(updated_sample).unwrap(),
        "{shape} update did not install the new source contents"
    );
    validate_materialization(&fixture.second);
    time_cold_end_to_end(fixture, shape, "copy-state-prune", "empty", Vec::new()).await;
    // `copy` deliberately leaves its writable materialization behind when it
    // disappears from the specification. Cleanup removes the state record;
    // only managed store symlinks are removed from the project tree.
    assert!(
        fs::symlink_metadata(sample).is_ok(),
        "copy-mode cleanup unexpectedly removed {}",
        sample.display()
    );
    let state: Value = serde_json::from_slice(&fs::read(&fixture.state).unwrap()).unwrap();
    assert_eq!(state["managedFiles"], json!([]));
}

fn assert_empty_state(state: &Path) {
    let state: Value = serde_json::from_slice(&fs::read(state).unwrap()).unwrap();
    assert_eq!(state["managedFiles"], json!([]));
    assert_no_deferred_cleanup_value(&state);
}

fn assert_no_deferred_cleanup(state: &Path) {
    let state: Value = serde_json::from_slice(&fs::read(state).unwrap()).unwrap();
    assert_no_deferred_cleanup_value(&state);
}

fn assert_no_deferred_cleanup_value(state: &Value) {
    assert!(
        state.get("garbage").is_none(),
        "cleanup journal was not pruned"
    );
}

fn deferred_cleanup_len(state: &Path) -> usize {
    let state: Value = serde_json::from_slice(&fs::read(state).unwrap()).unwrap();
    state
        .get("garbage")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn print_recovery_sidecar_size(state: &Path, shape: &str, operation: &str) {
    let recovery = state.with_extension("recovery");
    match fs::metadata(&recovery) {
        Ok(metadata) => eprintln!(
            "files-reconcile benchmark shape={shape} operation={operation} recovery_sidecar_bytes={}",
            metadata.len()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => eprintln!(
            "files-reconcile benchmark shape={shape} operation={operation} recovery_sidecar_bytes=absent"
        ),
        Err(error) => panic!(
            "failed to inspect recovery sidecar {}: {error}",
            recovery.display()
        ),
    }
}

async fn drain_deferred_cleanup(fixture: &Fixture, shape: &str, digest: &str, files: Vec<Value>) {
    let started = Instant::now();
    let mut passes = 0;
    while deferred_cleanup_len(&fixture.state) != 0 {
        assert!(passes < 128, "deferred cleanup did not converge");
        run(
            &fixture.root,
            &fixture.state,
            &fixture.cache,
            &format!("{shape}-bounded-cleanup-{passes}"),
            digest,
            files.clone(),
        )
        .await;
        passes += 1;
    }
    eprintln!(
        "files-reconcile benchmark timing=cold-end-to-end shape={shape} operation=bounded-cleanup-drain passes={passes} elapsed_us={}",
        started.elapsed().as_micros()
    );
}

fn tree_file_count_and_bytes(tree: &Path) -> (usize, u64) {
    let mut pending = vec![tree.to_path_buf()];
    let mut files = 0;
    let mut bytes = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                files += 1;
                bytes += metadata.len();
            } else {
                panic!(
                    "fixture tree contains an unsupported entry: {}",
                    entry.path().display()
                );
            }
        }
    }
    (files, bytes)
}

fn assert_tree_materialized(source: &Path, destination: &Path) {
    assert_eq!(
        tree_file_count_and_bytes(destination),
        tree_file_count_and_bytes(source),
        "destination tree did not materialize every source file"
    );
}

fn assert_many_directories_materialized(source: &Path, destination: &Path) {
    for directory in 0..MANY_DIRECTORIES {
        let name = format!("dir-{directory:04}");
        assert_tree_materialized(&source.join(&name), &destination.join(name));
    }
}

/// Includes create, unchanged, update, and copy-state pruning for both a flat
/// set of files and a nested tree. It also verifies the writable-copy metadata
/// contract.
#[tokio::test]
#[ignore = "manual performance benchmark"]
async fn benchmark_copy_lifecycle_matrix_for_flat_files_and_trees() {
    let flat = Fixture::flat();
    lifecycle_matrix(
        &flat,
        "flat",
        "update",
        flat_specs,
        &flat.root.join("flat/file-0000"),
        &flat.second,
        |_| {},
    )
    .await;

    let tree = Fixture::tree();
    lifecycle_matrix(
        &tree,
        "tree",
        "update",
        tree_specs,
        &tree.root.join("tree/dir-00/file-00"),
        &tree.second.join("dir-00/file-00"),
        |source| assert_tree_materialized(source, &tree.root.join("tree")),
    )
    .await;
}

/// Separates the initial state-file creation cost from the steady-state empty
/// reconciliation path. This must not accidentally gain managed entries.
#[tokio::test]
#[ignore = "manual empty-path performance benchmark"]
async fn benchmark_empty_reconciliation_paths() {
    let fixture = Fixture::flat();
    time_cold_end_to_end(&fixture, "empty", "create", "empty-v1", Vec::new()).await;
    assert_empty_state(&fixture.state);
    let state_after_create = fs::read(&fixture.state).unwrap();

    time_cold_end_to_end(&fixture, "empty", "unchanged", "empty-v1", Vec::new()).await;
    assert_eq!(fs::read(&fixture.state).unwrap(), state_after_create);
    assert_empty_state(&fixture.state);

    let prebuilt = build_task(
        &fixture.root,
        &fixture.state,
        &fixture.cache,
        "empty-prebuilt",
        "empty-v1",
        Vec::new(),
        Shutdown::new(),
    )
    .await;
    time_prebuilt_run(&prebuilt, "empty", "unchanged", "empty-prebuilt").await;
    assert_eq!(fs::read(&fixture.state).unwrap(), state_after_create);
    assert_empty_state(&fixture.state);
}

/// `copy` cleanup only prunes bookkeeping. This probe measures actual
/// filesystem deletion by creating managed symlinks and then removing them
/// from the desired set.
#[tokio::test]
#[ignore = "manual managed-symlink cleanup benchmark"]
async fn benchmark_real_managed_symlink_cleanup() {
    let fixture = Fixture::flat();
    let desired = symlink_specs(&fixture.first);
    time_cold_end_to_end(&fixture, "symlink-flat", "create", "links-v1", desired).await;
    assert_eq!(
        fs::read_link(fixture.root.join("links/file-0000")).unwrap(),
        fixture.first
    );

    time_cold_end_to_end(
        &fixture,
        "symlink-flat",
        "real-cleanup",
        "links-empty",
        Vec::new(),
    )
    .await;
    assert!(
        !fixture.root.join("links").exists(),
        "managed symlink cleanup left its parent directory behind"
    );
    assert_empty_state(&fixture.state);
}

/// Exercises an 8,192-file directory. Its update is deliberately labelled as
/// an end-to-end replacement followed by bounded cleanup drain measurements.
#[tokio::test]
#[ignore = "manual large-tree performance benchmark"]
async fn benchmark_large_tree_copy_lifecycle() {
    let fixture = Fixture::large_tree();
    lifecycle_matrix(
        &fixture,
        "large-tree",
        "update-with-bounded-cleanup",
        tree_specs,
        &fixture.root.join("tree/dir-00/file-00"),
        &fixture.second.join("dir-00/file-00"),
        |source| assert_tree_materialized(source, &fixture.root.join("tree")),
    )
    .await;
    drain_deferred_cleanup(&fixture, "large-tree", "empty", Vec::new()).await;
    assert_empty_state(&fixture.state);
}

/// Replaces many independent directory destinations in one invocation. The
/// update repeatedly records recovery intent and leaves more deferred removals
/// than one bounded-cleanup pass may process. Timings are cold end-to-end;
/// tree validation and journal inspection remain outside the timed sections.
#[tokio::test]
#[ignore = "manual many-directory recovery and cleanup benchmark"]
async fn benchmark_many_independent_directory_replacements() {
    let fixture = Fixture::many_directories();
    let shape = "many-independent-directories";

    time_cold_end_to_end(
        &fixture,
        shape,
        "create",
        "many-v1",
        many_directory_specs(&fixture.first),
    )
    .await;
    assert_many_directories_materialized(&fixture.first, &fixture.root.join("many"));
    print_recovery_sidecar_size(&fixture.state, shape, "create");

    time_cold_end_to_end(
        &fixture,
        shape,
        "unchanged",
        "many-v1",
        many_directory_specs(&fixture.first),
    )
    .await;
    assert_many_directories_materialized(&fixture.first, &fixture.root.join("many"));
    print_recovery_sidecar_size(&fixture.state, shape, "unchanged");

    time_cold_end_to_end(
        &fixture,
        shape,
        "update-with-recovery-journal",
        "many-v2",
        many_directory_specs(&fixture.second),
    )
    .await;
    assert_many_directories_materialized(&fixture.second, &fixture.root.join("many"));
    let deferred_after_update = deferred_cleanup_len(&fixture.state);
    assert!(
        deferred_after_update > 0 && deferred_after_update < MANY_DIRECTORIES,
        "the update must retain work for later bounded cleanup: {deferred_after_update} records"
    );
    print_recovery_sidecar_size(&fixture.state, shape, "update-with-recovery-journal");

    drain_deferred_cleanup(
        &fixture,
        shape,
        "many-v2",
        many_directory_specs(&fixture.second),
    )
    .await;
    assert_many_directories_materialized(&fixture.second, &fixture.root.join("many"));
    assert_no_deferred_cleanup(&fixture.state);
    print_recovery_sidecar_size(&fixture.state, shape, "bounded-cleanup-drain");
}

fn has_staged_replacement(root: &Path) -> bool {
    fs::read_dir(root).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".devenv-file-")
        })
    })
}

async fn wait_for_staged_replacement(root: &Path) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while !has_staged_replacement(root) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("large-tree replacement never exposed a staged destination");
}

/// Starts a contender after observing the updater's staged tree. This makes
/// contention likely but does not prove the precise lock-acquisition instant,
/// so the reported value is explicitly an estimate: contender wall time minus
/// an uncontended prebuilt unchanged baseline of the same tree shape.
#[tokio::test]
#[ignore = "manual concurrent contender-delay benchmark"]
async fn benchmark_large_tree_replacement_with_concurrent_contender_delay() {
    let fixture = Fixture::large_tree();
    time_cold_end_to_end(
        &fixture,
        "large-tree",
        "create",
        "large-v1",
        tree_specs(&fixture.first),
    )
    .await;
    assert_tree_materialized(&fixture.first, &fixture.root.join("tree"));

    let baseline = build_task(
        &fixture.root,
        &fixture.state,
        &fixture.cache,
        "large-uncontended-baseline",
        "large-v1",
        tree_specs(&fixture.first),
        Shutdown::new(),
    )
    .await;
    let uncontended_baseline = time_prebuilt_run(
        &baseline,
        "large-tree",
        "uncontended-unchanged-baseline",
        "large-uncontended-baseline",
    )
    .await;

    let updater = build_task(
        &fixture.root,
        &fixture.state,
        &fixture.cache,
        "large-updater",
        "large-v2",
        tree_specs(&fixture.second),
        Shutdown::new(),
    )
    .await;
    let contender = build_task(
        &fixture.root,
        &fixture.state,
        &fixture.cache,
        "large-contender",
        "large-v2",
        tree_specs(&fixture.second),
        Shutdown::new(),
    )
    .await;

    let updater_started = Instant::now();
    let mut updater_run = Box::pin(run_tasks(&updater, "large-updater"));
    tokio::select! {
        () = wait_for_staged_replacement(&fixture.root) => {}
        () = &mut updater_run => panic!("updater finished before the staged replacement was observed"),
    }

    let contender_started = Instant::now();
    let contender_run = run_tasks(&contender, "large-contender");
    tokio::join!(updater_run, contender_run);
    let contender_elapsed = contender_started.elapsed();
    let updater_elapsed = updater_started.elapsed();
    eprintln!(
        "files-reconcile benchmark timing=prebuilt-run shape=large-tree operation=concurrent-update contender_elapsed_us={} uncontended_baseline_us={} estimated_contender_delay_us={} updater_elapsed_us={}",
        contender_elapsed.as_micros(),
        uncontended_baseline.as_micros(),
        contender_elapsed
            .saturating_sub(uncontended_baseline)
            .as_micros(),
        updater_elapsed.as_micros(),
    );
    assert_eq!(
        fs::read(fixture.root.join("tree/dir-00/file-00")).unwrap(),
        fs::read(fixture.second.join("dir-00/file-00")).unwrap(),
    );
    assert_tree_materialized(&fixture.second, &fixture.root.join("tree"));
    drain_deferred_cleanup(
        &fixture,
        "large-tree-contention",
        "large-v2",
        tree_specs(&fixture.second),
    )
    .await;
    assert_no_deferred_cleanup(&fixture.state);
}

/// Measures the immutable-source fast path against a real store tree. The
/// caller chooses a representative closure with `DEVENV_FILES_STORE_SOURCE`.
#[tokio::test]
#[ignore = "manual Nix-store performance benchmark"]
async fn benchmark_real_nix_store_tree_create_and_unchanged() {
    let source = PathBuf::from(
        std::env::var_os("DEVENV_FILES_STORE_SOURCE")
            .expect("set DEVENV_FILES_STORE_SOURCE to a directory in /nix/store"),
    );
    let source = fs::canonicalize(source).unwrap();
    assert!(source.starts_with("/nix/store"));
    assert!(source.is_dir());

    let temp = tempfile::tempdir().unwrap();
    let fixture = Fixture {
        root: temp.path().join("project"),
        state: temp.path().join("state/files.json"),
        cache: temp.path().join("cache"),
        first: source.clone(),
        second: source,
        _temp: temp,
    };
    time_cold_end_to_end(
        &fixture,
        "nix-store-tree",
        "create-and-prove-immutable",
        "store",
        tree_specs(&fixture.first),
    )
    .await;
    let state: Value = serde_json::from_slice(&fs::read(&fixture.state).unwrap()).unwrap();
    assert_eq!(
        state["entries"]["tree"]["immutable_source"],
        json!(fixture.first)
    );
    let unchanged = build_task(
        &fixture.root,
        &fixture.state,
        &fixture.cache,
        "nix-store-unchanged",
        "store",
        tree_specs(&fixture.first),
        Shutdown::new(),
    )
    .await;
    time_prebuilt_run(
        &unchanged,
        "nix-store-tree",
        "unchanged-persisted-proof",
        "nix-store-unchanged",
    )
    .await;
}

/// Makes sparse-file allocation visible on each target filesystem. The v1
/// contract requires content and length preservation; allocation is reported
/// rather than asserted because reflink/extents support varies by filesystem.
#[tokio::test]
#[ignore = "manual sparse-file platform probe"]
async fn probe_sparse_copy_allocation_and_metadata() {
    let fixture = Fixture::flat();
    let sparse = fixture.first.with_file_name("sparse");
    let mut source = fs::File::create(&sparse).unwrap();
    source.seek(SeekFrom::Start(32 * 1024 * 1024)).unwrap();
    source.write_all(b"x").unwrap();
    drop(source);

    time_cold_end_to_end(
        &fixture,
        "sparse",
        "create",
        "sparse-v1",
        vec![spec("sparse", &sparse)],
    )
    .await;
    let destination = fixture.root.join("sparse");
    let source_meta = fs::metadata(&sparse).unwrap();
    let destination_meta = fs::metadata(&destination).unwrap();
    assert_eq!(destination_meta.len(), source_meta.len());
    let mut copied = fs::File::open(&destination).unwrap();
    copied.seek(SeekFrom::End(-1)).unwrap();
    let mut tail = [0; 1];
    copied.read_exact(&mut tail).unwrap();
    assert_eq!(tail, [b'x']);
    eprintln!(
        "files-reconcile sparse platform source_blocks={} destination_blocks={} source_dev={} destination_dev={}",
        source_meta.blocks(),
        destination_meta.blocks(),
        source_meta.dev(),
        destination_meta.dev(),
    );
}
