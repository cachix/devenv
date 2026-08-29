use devenv_tasks::{Config, Tasks};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use tokio_shutdown::Shutdown;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

const TASK_NAME: &str = "devenv:test:files";

fn file_spec(path: &str, source: &Path, mode: &str) -> Value {
    json!({
        "path": path,
        "source": source,
        "mode": mode,
    })
}

fn make_fallback(cache: &Path, tag: &str) -> PathBuf {
    fs::create_dir_all(cache).unwrap();
    let path = cache.join(format!("fallback-{tag}.sh"));
    fs::write(&path, "#!/bin/sh\nexit 97\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

async fn build_files_task(
    root: &Path,
    state_file: &Path,
    cache: &Path,
    tag: &str,
    digest: &str,
    files: Vec<Value>,
    shutdown: std::sync::Arc<Shutdown>,
) -> Tasks {
    let fallback = make_fallback(cache, tag);
    let config = Config::try_from(json!({
        "roots": [TASK_NAME],
        "run_mode": "all",
        "tasks": [{
            "name": TASK_NAME,
            "command": fallback,
            "builtin": {
                "name": "files-reconcile",
                "version": 1,
                "input": {
                    "root": root,
                    "state_file": state_file,
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

async fn run_files(
    root: &Path,
    state_file: &Path,
    cache: &Path,
    tag: &str,
    digest: &str,
    files: Vec<Value>,
) {
    let tasks =
        build_files_task(root, state_file, cache, tag, digest, files, Shutdown::new()).await;
    let outputs = tasks.run(false).await;
    let status = tasks.get_completion_status().await;
    assert_eq!(
        status.succeeded, 1,
        "native files task did not succeed: {status:?}; outputs={outputs:?}"
    );
    assert_eq!(status.failed, 0, "native files task failed");
    assert_eq!(status.cancelled, 0, "native files task was cancelled");
}

fn setup() -> (TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let sources = temp.path().join("sources");
    let state = temp.path().join("state/files.json");
    let cache = temp.path().join("cache");
    fs::create_dir_all(&sources).unwrap();
    (temp, root, sources, state, cache)
}

#[cfg(unix)]
#[tokio::test]
async fn copy_file_and_tree_create_unchanged_update_and_preserve_modes() {
    let (_temp, root, sources, state, cache) = setup();
    let source_file = sources.join("flat");
    let source_tree = sources.join("tree");
    fs::write(&source_file, "flat-v1").unwrap();
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o555)).unwrap();
    fs::create_dir_all(source_tree.join("nested")).unwrap();
    fs::write(source_tree.join("nested/value"), "tree-v1").unwrap();
    fs::set_permissions(
        source_tree.join("nested/value"),
        fs::Permissions::from_mode(0o444),
    )
    .unwrap();

    let desired = || {
        vec![
            file_spec("flat", &source_file, "copy"),
            file_spec("tree", &source_tree, "copy"),
        ]
    };
    run_files(&root, &state, &cache, "create", "v1", desired()).await;
    assert_eq!(fs::read_to_string(root.join("flat")).unwrap(), "flat-v1");
    assert_eq!(
        fs::read_to_string(root.join("tree/nested/value")).unwrap(),
        "tree-v1"
    );
    assert_eq!(
        fs::metadata(root.join("flat"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(root.join("tree/nested/value"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );

    let first_state = fs::read(&state).unwrap();
    let first_state_inode = fs::metadata(&state).unwrap().ino();
    run_files(&root, &state, &cache, "unchanged", "v1", desired()).await;
    assert_eq!(
        fs::read(&state).unwrap(),
        first_state,
        "an unchanged run should not rewrite reconciliation state"
    );
    assert_eq!(
        fs::metadata(&state).unwrap().ino(),
        first_state_inode,
        "an unchanged run should keep the existing state-file inode"
    );

    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&source_file, "flat-v2").unwrap();
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(
        source_tree.join("nested/value"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    fs::write(source_tree.join("nested/value"), "tree-v2").unwrap();
    fs::set_permissions(
        source_tree.join("nested/value"),
        fs::Permissions::from_mode(0o444),
    )
    .unwrap();
    run_files(&root, &state, &cache, "update", "v2", desired()).await;
    assert_eq!(fs::read_to_string(root.join("flat")).unwrap(), "flat-v2");
    assert_eq!(
        fs::read_to_string(root.join("tree/nested/value")).unwrap(),
        "tree-v2"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_file_and_tree_update_then_cleanup_exact_targets() {
    let (_temp, root, sources, state, cache) = setup();
    let first_file = sources.join("first-file");
    let second_file = sources.join("second-file");
    let first_tree = sources.join("first-tree");
    let second_tree = sources.join("second-tree");
    fs::write(&first_file, "first").unwrap();
    fs::write(&second_file, "second").unwrap();
    fs::create_dir(&first_tree).unwrap();
    fs::create_dir(&second_tree).unwrap();
    fs::write(first_tree.join("value"), "first").unwrap();
    fs::write(second_tree.join("value"), "second").unwrap();

    run_files(
        &root,
        &state,
        &cache,
        "links-create",
        "links-v1",
        vec![
            file_spec("links/flat", &first_file, "symlink"),
            file_spec("links/tree", &first_tree, "symlink"),
        ],
    )
    .await;

    run_files(
        &root,
        &state,
        &cache,
        "links-unchanged",
        "links-v1",
        vec![
            file_spec("links/flat", &first_file, "symlink"),
            file_spec("links/tree", &first_tree, "symlink"),
        ],
    )
    .await;

    run_files(
        &root,
        &state,
        &cache,
        "links-update",
        "links-v2",
        vec![
            file_spec("links/flat", &second_file, "symlink"),
            file_spec("links/tree", &second_tree, "symlink"),
        ],
    )
    .await;
    assert_eq!(fs::read_link(root.join("links/flat")).unwrap(), second_file);
    assert_eq!(fs::read_link(root.join("links/tree")).unwrap(), second_tree);

    run_files(&root, &state, &cache, "links-cleanup", "empty", vec![]).await;
    assert!(!root.join("links").exists());
    let state_json: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(state_json["managedFiles"], json!([]));
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_reconciliation_serializes_through_shared_state_lock() {
    let (_temp, root, sources, state, cache) = setup();
    let source_file = sources.join("flat");
    let source_tree = sources.join("tree");
    fs::write(&source_file, "flat").unwrap();
    fs::create_dir(&source_tree).unwrap();
    fs::write(source_tree.join("value"), "tree").unwrap();
    let desired = || {
        vec![
            file_spec("flat", &source_file, "copy"),
            file_spec("tree", &source_tree, "copy"),
        ]
    };

    let first = run_files(&root, &state, &cache, "concurrent-a", "same", desired());
    let second = run_files(&root, &state, &cache, "concurrent-b", "same", desired());
    tokio::join!(first, second);
    assert_eq!(fs::read_to_string(root.join("flat")).unwrap(), "flat");
    assert_eq!(fs::read_to_string(root.join("tree/value")).unwrap(), "tree");
    let state_json: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(state_json["managedFiles"], json!(["flat", "tree"]));
}

#[cfg(unix)]
#[tokio::test]
async fn sparse_copy_preserves_contents_and_reports_allocation_behavior() {
    use std::io::{Seek, SeekFrom, Write};

    let (_temp, root, sources, state, cache) = setup();
    let source = sources.join("sparse");
    let mut file = fs::File::create(&source).unwrap();
    file.seek(SeekFrom::Start(16 * 1024 * 1024)).unwrap();
    file.write_all(b"x").unwrap();
    drop(file);

    run_files(
        &root,
        &state,
        &cache,
        "sparse",
        "sparse-v1",
        vec![file_spec("sparse", &source, "copy")],
    )
    .await;

    let source_meta = fs::metadata(&source).unwrap();
    let destination_meta = fs::metadata(root.join("sparse")).unwrap();
    assert_eq!(destination_meta.len(), source_meta.len());
    let mut destination = fs::File::open(root.join("sparse")).unwrap();
    destination.seek(SeekFrom::End(-1)).unwrap();
    let mut tail = [0_u8; 1];
    use std::io::Read;
    destination.read_exact(&mut tail).unwrap();
    assert_eq!(tail, [b'x']);

    // APFS implements both cloning and SEEK_DATA/SEEK_HOLE. The destination
    // must remain sparse even when cloning falls back to extent copying.
    #[cfg(target_os = "macos")]
    if nix::sys::statfs::statfs(&root)
        .is_ok_and(|filesystem| filesystem.filesystem_type_name() == "apfs")
    {
        assert!(
            destination_meta.blocks() * 512 < destination_meta.len() / 64,
            "APFS copy unexpectedly materialized the sparse hole: blocks={} length={}",
            destination_meta.blocks(),
            destination_meta.len(),
        );
    }

    // This is diagnostic until sparse-file preservation becomes part of the
    // v1 contract. It makes platform behavior visible in `--nocapture` runs.
    eprintln!(
        "sparse allocation: source_blocks={} destination_blocks={}",
        source_meta.blocks(),
        destination_meta.blocks()
    );
}

#[tokio::test]
async fn cross_filesystem_copy_uses_a_safe_fallback() {
    // The volume matrix requires a cross-device source on both Linux and
    // macOS. Ordinary Linux runs can also exercise it using tmpfs.
    let configured = std::env::var_os("DEVENV_FILES_SOURCE_DIR");
    let source_root = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(Path::new("/dev/shm"));
    if !source_root.is_dir() {
        assert!(
            configured.is_none(),
            "configured cross-filesystem source is missing"
        );
        eprintln!("skipping cross-filesystem copy: no source volume configured");
        return;
    }

    let (_temp, root, _sources, state, cache) = setup();
    fs::create_dir_all(&root).unwrap();
    let source_temp = tempfile::Builder::new()
        .prefix("devenv-files-source-")
        .tempdir_in(source_root)
        .unwrap();
    if fs::metadata(source_temp.path()).unwrap().dev() == fs::metadata(&root).unwrap().dev() {
        assert!(
            configured.is_none(),
            "matrix source and destination must be on different devices"
        );
        eprintln!("skipping cross-filesystem copy: source shares the destination device");
        return;
    }

    let source = source_temp.path().join("source");
    let contents = vec![b'x'; 2 * 1024 * 1024];
    fs::write(&source, &contents).unwrap();
    run_files(
        &root,
        &state,
        &cache,
        "cross-filesystem",
        "cross-filesystem-v1",
        vec![file_spec("copied", &source, "copy")],
    )
    .await;

    assert_eq!(fs::read(root.join("copied")).unwrap(), contents);
}

#[tokio::test]
async fn respects_the_destination_volume_case_behavior() {
    let (_temp, root, sources, state, cache) = setup();
    let first = sources.join("first");
    let second = sources.join("second");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    fs::create_dir_all(&root).unwrap();

    let probe = root.join("devenv-case-probe");
    fs::write(&probe, "probe").unwrap();
    let case_insensitive = root.join("DEVENV-CASE-PROBE").exists();
    fs::remove_file(&probe).unwrap();

    let desired = vec![
        file_spec("CaseName", &first, "copy"),
        file_spec("casename", &second, "copy"),
    ];
    if case_insensitive {
        let tasks = build_files_task(
            &root,
            &state,
            &cache,
            "case-insensitive",
            "case-insensitive-v1",
            desired,
            Shutdown::new(),
        )
        .await;
        let _ = tasks.run(false).await;
        let status = tasks.get_completion_status().await;
        assert_eq!(
            status.succeeded, 0,
            "case-folded destinations must not both succeed"
        );
        assert_eq!(
            status.failed, 1,
            "case-folded destination collision was not reported"
        );
    } else {
        run_files(
            &root,
            &state,
            &cache,
            "case-sensitive",
            "case-sensitive-v1",
            desired,
        )
        .await;
        assert_eq!(fs::read_to_string(root.join("CaseName")).unwrap(), "first");
        assert_eq!(fs::read_to_string(root.join("casename")).unwrap(), "second");
    }
}

/// Slow stress coverage for cancellation after native mutation has begun.
/// Run explicitly with:
/// `cargo test -p devenv-tasks --test files_builtin_integration -- --ignored`
#[cfg(unix)]
#[tokio::test]
#[ignore = "slow cancellation stress test"]
async fn cancellation_stops_reconciliation_and_persists_only_completed_work() {
    let (_temp, root, sources, state, cache) = setup();
    let source = sources.join("payload");
    fs::write(&source, vec![0x5a; 256 * 1024]).unwrap();
    let total = 1024_usize;
    let desired = (0..total)
        .map(|index| file_spec(&format!("file-{index:04}"), &source, "copy"))
        .collect();
    let shutdown = Shutdown::new();
    let tasks = build_files_task(
        &root,
        &state,
        &cache,
        "cancel",
        "cancel-v1",
        desired,
        shutdown.clone(),
    )
    .await;

    let run = tasks.run(false);
    let cancel = async {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if fs::read_dir(&root)
                    .ok()
                    .and_then(|mut entries| entries.next())
                    .is_some()
                {
                    shutdown.shutdown();
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("native reconciliation did not begin in time");
    };
    let (_outputs, ()) = tokio::join!(run, cancel);

    let state_json = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(bytes) = fs::read(&state)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled worker did not persist partial state");
    let completed = state_json["managedFiles"].as_array().unwrap().len();
    assert!(
        completed < total,
        "cancellation did not stop reconciliation"
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), completed);
}
