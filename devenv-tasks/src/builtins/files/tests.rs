use super::*;
use std::io;
use std::os::unix::fs::{MetadataExt, symlink};

const TEST_CLEANUP_STEP_BUDGET: usize = 256;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for CapturedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturedWriter {
    type Writer = CapturedWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn capture_warnings<T>(f: impl FnOnce() -> T) -> (T, String) {
    let writer = CapturedWriter::default();
    let bytes = writer.0.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(writer)
        .finish();
    let result = tracing::subscriber::with_default(subscriber, f);
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    (result, output)
}

fn read_state(path: &Path) -> Result<FilesState> {
    let state_directory = StateDirectory::new(path).into_diagnostic()?;
    read_state_anchored(&state_directory)
}

fn desired(path: &str, source: &Path, mode: CopyMode) -> DesiredFile {
    DesiredFile {
        path: path.to_string(),
        source: source.to_path_buf(),
        mode,
    }
}

fn input(root: &Path, state_file: &Path, digest: &str, files: Vec<DesiredFile>) -> FilesInputV1 {
    FilesInputV1 {
        root: root.to_path_buf(),
        state_file: state_file.to_path_buf(),
        desired_digest: digest.to_string(),
        files,
    }
}

fn run(input: FilesInputV1) -> Result<Counts> {
    reconcile(input, CancellationToken::new())
}

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let sources = temp.path().join("sources");
    let state = temp.path().join("state/files.json");
    fs::create_dir_all(&sources).unwrap();
    (temp, root, sources, state)
}

fn artifact_map(
    artifacts: impl IntoIterator<Item = RecoveryArtifact>,
) -> BTreeMap<String, RecoveryArtifact> {
    artifacts
        .into_iter()
        .map(|artifact| (artifact.id.clone(), artifact))
        .collect()
}

fn fallback_map(
    fallbacks: impl IntoIterator<Item = FallbackRecovery>,
) -> BTreeMap<String, FallbackRecovery> {
    fallbacks
        .into_iter()
        .map(|fallback| (fallback.stage_id.clone(), fallback))
        .collect()
}

fn recovery_artifact(id: String, phase: ArtifactPhase) -> RecoveryArtifact {
    RecoveryArtifact {
        intent: ArtifactIntent {
            relative_parent: PathBuf::new(),
            parent_device: 1,
            parent_inode: 2,
            name: format!(".devenv-files-artifact-{id}"),
        },
        id,
        artifact: None,
        phase,
    }
}

fn fallback_recovery(
    stage_id: String,
    backup_id: String,
    phase: FallbackPhase,
) -> FallbackRecovery {
    let identity = EntryIdentity {
        device: 1,
        inode: 2,
        kind: EntryKind::RegularFile,
    };
    FallbackRecovery {
        destination: "target".to_string(),
        stage_id,
        backup_id,
        original: identity,
        staged: identity,
        placeholder: identity,
        phase,
    }
}

fn deferred_for(root: &Path, relative: &str) -> Deferred {
    let path = root.join(relative);
    let parent = fs::metadata(path.parent().unwrap()).unwrap();
    let entry = fs::symlink_metadata(&path).unwrap();
    Deferred {
        relative_parent: Path::new(relative)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf(),
        parent_device: parent.dev(),
        parent_inode: parent.ino(),
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        device: entry.dev(),
        inode: entry.ino(),
    }
}

fn write_test_state(path: &Path, state: &FilesState) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let directory = StateDirectory::new(path).unwrap();
    write_state(&directory, state).unwrap();
}

#[cfg(unix)]
#[test]
fn symlink_create_noop_update_and_conflict() {
    let (_temp, root, sources, state) = setup();
    let first = sources.join("first");
    let second = sources.join("second");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();

    let counts = run(input(
        &root,
        &state,
        "one",
        vec![desired("nested/file", &first, CopyMode::Symlink)],
    ))
    .unwrap();
    assert_eq!(counts.created, 1);
    assert_eq!(fs::read_link(root.join("nested/file")).unwrap(), first);

    let counts = run(input(
        &root,
        &state,
        "one",
        vec![desired("nested/file", &first, CopyMode::Symlink)],
    ))
    .unwrap();
    assert_eq!(counts.unchanged, 1);

    let counts = run(input(
        &root,
        &state,
        "two",
        vec![desired("nested/file", &second, CopyMode::Symlink)],
    ))
    .unwrap();
    assert_eq!(counts.updated, 1);
    assert_eq!(fs::read_link(root.join("nested/file")).unwrap(), second);

    fs::remove_file(root.join("nested/file")).unwrap();
    fs::write(root.join("nested/file"), "user-owned").unwrap();
    let (counts, warnings) = capture_warnings(|| {
        run(input(
            &root,
            &state,
            "three",
            vec![desired("nested/file", &first, CopyMode::Symlink)],
        ))
        .unwrap()
    });
    assert_eq!(counts.conflicting, 1);
    assert!(warnings.contains("conflicting managed file"), "{warnings}");
    assert!(warnings.contains("nested/file"), "{warnings}");
    assert_eq!(
        fs::read_to_string(root.join("nested/file")).unwrap(),
        "user-owned"
    );
    assert!(
        !read_state(&state)
            .unwrap()
            .managed_files
            .contains(&"nested/file".into())
    );
}

#[test]
fn seed_materializes_once_and_retains_edits() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("seed");
    fs::write(&source, "initial").unwrap();

    let counts = run(input(
        &root,
        &state,
        "one",
        vec![desired("config", &source, CopyMode::Seed)],
    ))
    .unwrap();
    assert_eq!(counts.created, 1);
    fs::write(root.join("config"), "user edit").unwrap();
    fs::write(&source, "new default").unwrap();

    let (counts, warnings) = capture_warnings(|| {
        run(input(
            &root,
            &state,
            "two",
            vec![desired("config", &source, CopyMode::Seed)],
        ))
        .unwrap()
    });
    assert_eq!(counts.retained, 1);
    assert!(
        warnings.contains("retaining existing managed path"),
        "{warnings}"
    );
    assert!(warnings.contains("config"), "{warnings}");
    assert_eq!(
        fs::read_to_string(root.join("config")).unwrap(),
        "user edit"
    );
}

#[cfg(unix)]
#[test]
fn seed_replaces_a_previously_managed_symlink() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("seed");
    fs::write(&source, "materialized").unwrap();

    run(input(
        &root,
        &state,
        "link",
        vec![desired("config", &source, CopyMode::Symlink)],
    ))
    .unwrap();
    let counts = run(input(
        &root,
        &state,
        "seed",
        vec![desired("config", &source, CopyMode::Seed)],
    ))
    .unwrap();

    assert_eq!(counts.updated, 1);
    assert!(
        !fs::symlink_metadata(root.join("config"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(root.join("config")).unwrap(),
        "materialized"
    );
}

#[cfg(unix)]
#[test]
fn symlink_replaces_an_unchanged_managed_copy() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "managed").unwrap();

    run(input(
        &root,
        &state,
        "copy",
        vec![desired("config", &source, CopyMode::Copy)],
    ))
    .unwrap();
    let counts = run(input(
        &root,
        &state,
        "symlink",
        vec![desired("config", &source, CopyMode::Symlink)],
    ))
    .unwrap();

    assert_eq!(counts.updated, 1);
    assert_eq!(fs::read_link(root.join("config")).unwrap(), source);
    let state = read_state(&state).unwrap();
    let entry = state.entries.get("config").unwrap();
    assert_eq!(entry.mode, Some(CopyMode::Symlink));
    assert!(entry.fingerprint.is_none());
}

#[cfg(unix)]
#[test]
fn symlink_replaces_an_unchanged_managed_copy_directory() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("value"), "managed").unwrap();

    run(input(
        &root,
        &state,
        "copy",
        vec![desired("config", &source, CopyMode::Copy)],
    ))
    .unwrap();
    let counts = run(input(
        &root,
        &state,
        "symlink",
        vec![desired("config", &source, CopyMode::Symlink)],
    ))
    .unwrap();

    assert_eq!(counts.updated, 1);
    assert_eq!(fs::read_link(root.join("config")).unwrap(), source);
    assert_eq!(
        read_state(&state)
            .unwrap()
            .entries
            .get("config")
            .unwrap()
            .mode,
        Some(CopyMode::Symlink)
    );
}

#[cfg(unix)]
#[test]
fn symlink_retains_a_user_modified_managed_copy_and_its_state() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "managed").unwrap();

    run(input(
        &root,
        &state,
        "copy",
        vec![desired("config", &source, CopyMode::Copy)],
    ))
    .unwrap();
    let previous = read_state(&state)
        .unwrap()
        .entries
        .remove("config")
        .unwrap();
    fs::write(root.join("config"), "user edit").unwrap();

    let counts = run(input(
        &root,
        &state,
        "symlink",
        vec![desired("config", &source, CopyMode::Symlink)],
    ))
    .unwrap();

    assert_eq!(counts.conflicting, 1);
    assert_eq!(
        fs::read_to_string(root.join("config")).unwrap(),
        "user edit"
    );
    assert_eq!(
        read_state(&state).unwrap().entries.get("config"),
        Some(&previous)
    );
}

#[cfg(unix)]
#[test]
fn symlink_retains_a_user_modified_managed_copy_directory() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("value"), "managed").unwrap();

    run(input(
        &root,
        &state,
        "copy",
        vec![desired("config", &source, CopyMode::Copy)],
    ))
    .unwrap();
    let previous = read_state(&state)
        .unwrap()
        .entries
        .remove("config")
        .unwrap();
    fs::write(root.join("config/value"), "user edit").unwrap();

    let counts = run(input(
        &root,
        &state,
        "symlink",
        vec![desired("config", &source, CopyMode::Symlink)],
    ))
    .unwrap();

    assert_eq!(counts.conflicting, 1);
    assert_eq!(
        fs::read_to_string(root.join("config/value")).unwrap(),
        "user edit"
    );
    assert_eq!(
        read_state(&state).unwrap().entries.get("config"),
        Some(&previous)
    );
}

#[cfg(unix)]
#[test]
fn symlink_does_not_replace_a_copy_without_a_recorded_fingerprint() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "managed").unwrap();

    run(input(
        &root,
        &state,
        "copy",
        vec![desired("config", &source, CopyMode::Copy)],
    ))
    .unwrap();
    let mut previous = read_state(&state).unwrap();
    previous.entries.get_mut("config").unwrap().fingerprint = None;
    write_test_state(&state, &previous);

    let counts = run(input(
        &root,
        &state,
        "symlink",
        vec![desired("config", &source, CopyMode::Symlink)],
    ))
    .unwrap();

    assert_eq!(counts.conflicting, 1);
    assert!(
        !fs::symlink_metadata(root.join("config"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        read_state(&state)
            .unwrap()
            .entries
            .get("config")
            .unwrap()
            .mode,
        Some(CopyMode::Copy)
    );
}

#[cfg(unix)]
#[test]
fn copy_noop_update_permissions_and_tree() {
    let (_temp, root, sources, state) = setup();
    let source_file = sources.join("file");
    fs::write(&source_file, "one").unwrap();
    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o444)).unwrap();
    let source_tree = sources.join("tree");
    fs::create_dir(&source_tree).unwrap();
    fs::create_dir(source_tree.join("nested")).unwrap();
    fs::write(source_tree.join("nested/value"), "tree").unwrap();

    let desired_files = || {
        vec![
            desired("file", &source_file, CopyMode::Copy),
            desired("tree", &source_tree, CopyMode::Copy),
        ]
    };
    let counts = run(input(&root, &state, "one", desired_files())).unwrap();
    assert_eq!(counts.created, 2);
    assert_eq!(
        fs::read_to_string(root.join("tree/nested/value")).unwrap(),
        "tree"
    );
    assert_eq!(
        fs::metadata(root.join("file"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );

    let counts = run(input(&root, &state, "one", desired_files())).unwrap();
    assert_eq!(counts.unchanged, 2);

    fs::set_permissions(&source_file, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&source_file, "two").unwrap();
    fs::write(source_tree.join("nested/value"), "changed").unwrap();
    let counts = run(input(&root, &state, "two", desired_files())).unwrap();
    assert_eq!(counts.updated, 2);
    assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "two");
    assert_eq!(
        fs::read_to_string(root.join("tree/nested/value")).unwrap(),
        "changed"
    );
    assert!(read_state(&state).unwrap().garbage.is_empty());

    let counts = run(input(&root, &state, "two", desired_files())).unwrap();
    assert_eq!(counts.unchanged, 2);
    assert!(read_state(&state).unwrap().garbage.is_empty());
}

#[test]
fn copied_directory_reuses_its_staged_tree_fingerprint() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("tree");
    fs::create_dir(&source).unwrap();
    // Create siblings in reverse lexical order to ensure the accumulator
    // matches the deterministic ordering of a fresh descriptor walk.
    fs::write(source.join("z-last"), "z").unwrap();
    fs::write(source.join("a-first"), "a").unwrap();
    fs::create_dir(source.join("nested")).unwrap();
    fs::write(source.join("nested/z-last"), "first").unwrap();
    fs::write(source.join("nested/a-first"), "first").unwrap();
    let linked_source = sources.join("linked-source");
    fs::create_dir(&linked_source).unwrap();
    fs::write(linked_source.join("value"), "followed").unwrap();
    symlink(&linked_source, source.join("m-linked")).unwrap();
    let files = || vec![desired("tree", &source, CopyMode::Copy)];

    fingerprint::reset_descriptor_tree_walk_count();
    let counts = run(input(&root, &state, "one", files())).unwrap();
    assert_eq!(counts.created, 1);
    assert_eq!(
        fingerprint::descriptor_tree_walk_count(),
        0,
        "creating a copied directory must not walk the committed tree"
    );
    let cached = read_state(&state)
        .unwrap()
        .entries
        .get("tree")
        .unwrap()
        .fingerprint
        .clone()
        .unwrap();
    let fresh = Resolver::new(&root)
        .unwrap()
        .destination("tree", false)
        .unwrap()
        .fingerprint(&CancellationToken::new())
        .unwrap();
    assert_eq!(cached, fresh);
    assert_eq!(
        fs::read_to_string(root.join("tree/m-linked/value")).unwrap(),
        "followed"
    );

    fingerprint::reset_descriptor_tree_walk_count();
    let counts = run(input(&root, &state, "one", files())).unwrap();
    assert_eq!(counts.unchanged, 1);
    assert_eq!(fingerprint::descriptor_tree_walk_count(), 1);

    fs::write(source.join("nested/z-last"), "source update").unwrap();
    fingerprint::reset_descriptor_tree_walk_count();
    let counts = run(input(&root, &state, "two", files())).unwrap();
    assert_eq!(counts.updated, 1);
    assert_eq!(
        fingerprint::descriptor_tree_walk_count(),
        1,
        "updating a copied directory should only perform the pre-update validation walk"
    );
    let cached = read_state(&state)
        .unwrap()
        .entries
        .get("tree")
        .unwrap()
        .fingerprint
        .clone()
        .unwrap();
    let fresh = Resolver::new(&root)
        .unwrap()
        .destination("tree", false)
        .unwrap()
        .fingerprint(&CancellationToken::new())
        .unwrap();
    assert_eq!(cached, fresh);

    fs::write(root.join("tree/nested/z-last"), "user edit").unwrap();
    fingerprint::reset_descriptor_tree_walk_count();
    let counts = run(input(&root, &state, "two", files())).unwrap();
    assert_eq!(counts.updated, 1);
    assert_eq!(fingerprint::descriptor_tree_walk_count(), 1);
    assert_eq!(
        fs::read_to_string(root.join("tree/nested/z-last")).unwrap(),
        "source update"
    );
}

#[test]
fn deferred_cleanup_backlog_bounds_directory_replacements() {
    let (_temp, root, sources, state) = setup();
    let source_tree = sources.join("tree");
    fs::create_dir(&source_tree).unwrap();
    fs::write(source_tree.join("value"), "first").unwrap();
    let files = || vec![desired("tree", &source_tree, CopyMode::Copy)];
    run(input(&root, &state, "one", files())).unwrap();

    let mut state_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    state_json["garbage"] = serde_json::Value::Array(
        (0..MAX_DEFERRED_CLEANUPS)
            .map(|index| {
                serde_json::json!({
                    "relative_parent": "",
                    "parent_device": 0,
                    "parent_inode": 0,
                    "name": format!(".devenv-old-{index}"),
                    "device": 0,
                    "inode": 0,
                })
            })
            .collect(),
    );
    fs::write(&state, serde_json::to_vec(&state_json).unwrap()).unwrap();
    fs::write(source_tree.join("value"), "second").unwrap();

    let error = run(input(&root, &state, "two", files())).unwrap_err();
    assert!(format!("{error:#}").contains("cleanup backlog reached"));
    assert_eq!(
        fs::read_to_string(root.join("tree/value")).unwrap(),
        "first"
    );
    assert_eq!(
        read_state(&state).unwrap().garbage.len(),
        MAX_DEFERRED_CLEANUPS - MAX_DEFERRED_CLEANUP_RECORDS_PER_RUN,
        "one run must inspect only its cleanup-record budget"
    );

    let mut current = read_state(&state).unwrap();
    current.garbage.clear();
    write_test_state(&state, &current);
    let counts = run(input(&root, &state, "two", files())).unwrap();
    assert_eq!(counts.updated, 1);
    assert_eq!(
        fs::read_to_string(root.join("tree/value")).unwrap(),
        "second"
    );
}

#[test]
fn duplicate_cleanup_records_do_not_consume_the_backlog() {
    let (_temp, root, sources, state) = setup();
    let source_tree = sources.join("tree");
    fs::create_dir(&source_tree).unwrap();
    fs::write(source_tree.join("value"), "first").unwrap();
    let files = || vec![desired("tree", &source_tree, CopyMode::Copy)];
    run(input(&root, &state, "one", files())).unwrap();

    let duplicate = serde_json::json!({
        "relative_parent": "",
        "parent_device": 0,
        "parent_inode": 0,
        "name": ".devenv-old-duplicate",
        "device": 0,
        "inode": 0,
    });
    let mut state_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    state_json["garbage"] = serde_json::Value::Array(vec![duplicate; MAX_DEFERRED_CLEANUPS]);
    fs::write(&state, serde_json::to_vec(&state_json).unwrap()).unwrap();
    fs::write(source_tree.join("value"), "second").unwrap();

    let counts = run(input(&root, &state, "two", files())).unwrap();
    assert_eq!(counts.updated, 1);
    assert!(read_state(&state).unwrap().garbage.is_empty());
}

#[cfg(unix)]
#[test]
fn cleanup_resolution_errors_remain_bounded_and_retryable() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "value").unwrap();
    let files = || vec![desired("value", &source, CopyMode::Copy)];
    run(input(&root, &state, "one", files())).unwrap();

    let blocked_parent = root.join("blocked-parent");
    let saved_parent = root.join("saved-parent");
    let blocked = blocked_parent.join(".devenv-old-blocked");
    fs::create_dir(&blocked_parent).unwrap();
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("child"), "value").unwrap();
    let deferred = deferred_for(&root, "blocked-parent/.devenv-old-blocked");
    fs::rename(&blocked_parent, &saved_parent).unwrap();
    std::os::unix::fs::symlink(&saved_parent, &blocked_parent).unwrap();
    let mut current = read_state(&state).unwrap();
    current.garbage = vec![deferred];
    write_test_state(&state, &current);

    for _ in 0..3 {
        run(input(&root, &state, "one", files())).unwrap();
        assert_eq!(read_state(&state).unwrap().garbage.len(), 1);
    }

    fs::remove_file(&blocked_parent).unwrap();
    fs::rename(&saved_parent, &blocked_parent).unwrap();
    run(input(&root, &state, "one", files())).unwrap();
    assert!(read_state(&state).unwrap().garbage.is_empty());
    assert!(!blocked.exists());
}

#[test]
fn cleanup_pruning_preserves_newer_records() {
    let (_temp, _root, _sources, state_path) = setup();
    let record = |name: &str, inode| Deferred {
        relative_parent: PathBuf::new(),
        parent_device: 1,
        parent_inode: 2,
        name: name.to_string(),
        device: 3,
        inode,
    };
    let resolved = record("resolved", 4);
    let newer = record("newer", 5);
    write_test_state(
        &state_path,
        &FilesState {
            version: 1,
            desired_digest: "same".to_string(),
            managed_files: Vec::new(),
            entries: BTreeMap::new(),
            garbage: vec![resolved.clone(), newer.clone()],
            orphan_sweeps: Vec::new(),
        },
    );
    let state_directory = StateDirectory::new(&state_path).unwrap();
    let mut lock = RwLock::new(state_directory.open_lock().unwrap());

    update_garbage_after_cleanup(
        &state_directory,
        &mut lock,
        std::slice::from_ref(&resolved),
        std::slice::from_ref(&resolved),
        &CancellationToken::new(),
    );

    assert_eq!(read_state(&state_path).unwrap().garbage, vec![newer]);
}

#[test]
fn cleanup_rotation_is_fifo_and_preserves_newer_records() {
    let (_temp, _root, _sources, state_path) = setup();
    let record = |name: &str, inode| Deferred {
        relative_parent: PathBuf::new(),
        parent_device: 1,
        parent_inode: 2,
        name: name.to_string(),
        device: 3,
        inode,
    };
    let first = record("first", 4);
    let second = record("second", 5);
    let newer = record("newer", 6);
    write_test_state(
        &state_path,
        &FilesState {
            version: 1,
            desired_digest: "same".to_string(),
            managed_files: Vec::new(),
            entries: BTreeMap::new(),
            garbage: vec![first.clone(), second.clone(), newer.clone()],
            orphan_sweeps: Vec::new(),
        },
    );
    let state_directory = StateDirectory::new(&state_path).unwrap();
    let mut lock = RwLock::new(state_directory.open_lock().unwrap());

    update_garbage_after_cleanup(
        &state_directory,
        &mut lock,
        &[first.clone(), second.clone()],
        &[],
        &CancellationToken::new(),
    );

    assert_eq!(
        read_state(&state_path).unwrap().garbage,
        vec![newer, first, second]
    );
}

#[test]
fn recursive_cleanup_step_ceiling_is_bounded_and_resumes() {
    let (_temp, root, _sources, _state) = setup();
    fs::create_dir_all(&root).unwrap();

    let old = root.join(".devenv-old-large");
    fs::create_dir(&old).unwrap();
    let child_count = TEST_CLEANUP_STEP_BUDGET + 20;
    for index in 0..child_count {
        fs::write(old.join(index.to_string()), "value").unwrap();
    }
    let deferred = deferred_for(&root, ".devenv-old-large");
    let mut resolver = Resolver::new(&root).unwrap();
    assert_eq!(
        resolver
            .remove_deferred_bounded(
                &deferred,
                &CancellationToken::new(),
                TEST_CLEANUP_STEP_BUDGET,
                Duration::MAX,
            )
            .unwrap(),
        DeferredRemoval::Incomplete
    );
    let remaining = fs::read_dir(&old).unwrap().count();
    assert!(
        remaining > 0,
        "one cleanup run must not drain an oversized tree"
    );
    assert!(
        child_count - remaining <= TEST_CLEANUP_STEP_BUDGET,
        "cleanup exceeded its unlink/rmdir work budget"
    );
    assert_eq!(
        resolver
            .remove_deferred_bounded(
                &deferred,
                &CancellationToken::new(),
                usize::MAX,
                Duration::MAX,
            )
            .unwrap(),
        DeferredRemoval::Removed
    );
    assert!(!old.exists());
}

#[test]
fn state_loss_reclaims_a_large_orphaned_artifact_beyond_the_former_step_budget() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "value").unwrap();
    fs::create_dir_all(root.join("nested")).unwrap();

    let artifact = root
        .join("nested")
        .join(format!(".devenv-files-artifact-{}", Uuid::new_v4()));
    fs::create_dir(&artifact).unwrap();
    fs::set_permissions(&artifact, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(artifact.join("payload")).unwrap();
    for index in 0..10_000 {
        fs::write(artifact.join("payload").join(index.to_string()), "value").unwrap();
    }

    run(input(
        &root,
        &state,
        "rebuild",
        vec![desired("nested/managed", &source, CopyMode::Copy)],
    ))
    .unwrap();

    if artifact.exists() {
        let remaining = fs::read_dir(artifact.join("payload")).unwrap().count();
        assert!(
            remaining < 10_000 - 256,
            "the time budget should make substantially more progress than the former 256-step cap"
        );
        assert_eq!(read_state(&state).unwrap().garbage.len(), 1);
    }
    for _ in 0..8 {
        if !artifact.exists() {
            break;
        }
        run(input(
            &root,
            &state,
            "rebuild",
            vec![desired("nested/managed", &source, CopyMode::Copy)],
        ))
        .unwrap();
    }
    assert!(
        !artifact.exists(),
        "the persisted cleanup identity must converge"
    );
    assert!(read_state(&state).unwrap().garbage.is_empty());
}

#[test]
fn orphaned_artifacts_are_reclaimed_when_state_is_malformed_or_legacy() {
    for contents in ["not json", r#"{"managedFiles":[],"desiredDigest":"old"}"#] {
        let (_temp, root, sources, state) = setup();
        let source = sources.join("source");
        fs::write(&source, "value").unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(&state, contents).unwrap();

        let artifact = root.join(format!(".devenv-files-artifact-{}", Uuid::new_v4()));
        fs::create_dir(&artifact).unwrap();
        fs::set_permissions(&artifact, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(artifact.join("payload"), "orphaned").unwrap();

        run(input(
            &root,
            &state,
            "rebuild",
            vec![desired("managed", &source, CopyMode::Copy)],
        ))
        .unwrap();

        assert!(!artifact.exists(), "state contents: {contents}");
        assert!(read_state(&state).unwrap().garbage.is_empty());
    }
}

#[test]
fn orphan_sweep_cursor_advances_past_more_than_one_run_of_artifacts() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "value").unwrap();
    fs::create_dir_all(&root).unwrap();
    let artifacts = (0..(MAX_ORPHAN_ARTIFACTS_PER_RUN + 1))
        .map(|_| {
            let artifact = root.join(format!(".devenv-files-artifact-{}", Uuid::new_v4()));
            fs::create_dir(&artifact).unwrap();
            fs::set_permissions(&artifact, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(artifact.join("payload"), "orphaned").unwrap();
            artifact
        })
        .collect::<Vec<_>>();
    let files = || vec![desired("managed", &source, CopyMode::Copy)];

    run(input(&root, &state, "rebuild", files())).unwrap();
    let first = read_state(&state).unwrap();
    assert_eq!(first.orphan_sweeps.len(), 1);
    assert_eq!(
        first.orphan_sweeps[0].artifacts.len(),
        MAX_ORPHAN_ARTIFACTS_PER_RUN
    );
    assert!(artifacts.iter().all(|artifact| artifact.exists()));

    run(input(&root, &state, "rebuild", files())).unwrap();
    assert!(read_state(&state).unwrap().orphan_sweeps.is_empty());
    for _ in 0..4 {
        if artifacts.iter().all(|artifact| !artifact.exists()) {
            break;
        }
        run(input(&root, &state, "rebuild", files())).unwrap();
    }
    assert!(artifacts.iter().all(|artifact| !artifact.exists()));
}

#[cfg(unix)]
#[test]
fn orphaned_artifact_sweep_never_follows_a_matching_symlink() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    let outside = sources.join("outside");
    fs::write(&source, "value").unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "user-owned").unwrap();
    let artifact = root.join(format!(".devenv-files-artifact-{}", Uuid::new_v4()));
    symlink(&outside, &artifact).unwrap();

    run(input(
        &root,
        &state,
        "rebuild",
        vec![desired("managed", &source, CopyMode::Copy)],
    ))
    .unwrap();

    assert!(artifact.is_symlink());
    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap(),
        "user-owned"
    );
}

#[cfg(unix)]
#[test]
fn orphaned_artifact_sweep_preserves_uuid_named_user_directories() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "value").unwrap();
    fs::create_dir_all(&root).unwrap();
    let user_directory = root.join(format!(".devenv-files-artifact-{}", Uuid::new_v4()));
    fs::create_dir(&user_directory).unwrap();
    fs::set_permissions(&user_directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(user_directory.join("notes"), "user-owned").unwrap();

    run(input(
        &root,
        &state,
        "rebuild",
        vec![desired("managed", &source, CopyMode::Copy)],
    ))
    .unwrap();

    assert_eq!(
        fs::read_to_string(user_directory.join("notes")).unwrap(),
        "user-owned"
    );
}

#[test]
fn deep_cleanup_step_ceiling_still_reaches_a_leaf() {
    let (_temp, root, _sources, _state) = setup();
    fs::create_dir_all(&root).unwrap();

    // Still deeper than the removal budget, but fits macOS's default
    // 256-descriptor limit (the traversal holds descriptors per level).
    const STEP_BUDGET: usize = 32;
    let old = root.join(".devenv-old-deep");
    let mut leaf_parent = old.clone();
    for _ in 0..(STEP_BUDGET + 20) {
        fs::create_dir(&leaf_parent).unwrap();
        leaf_parent.push("d");
    }
    fs::create_dir(&leaf_parent).unwrap();
    let leaf = leaf_parent.join("leaf");
    fs::write(&leaf, "value").unwrap();
    let deferred = deferred_for(&root, ".devenv-old-deep");
    let mut resolver = Resolver::new(&root).unwrap();
    assert_eq!(
        resolver
            .remove_deferred_bounded(
                &deferred,
                &CancellationToken::new(),
                STEP_BUDGET,
                Duration::MAX,
            )
            .unwrap(),
        DeferredRemoval::Incomplete
    );
    assert!(
        !leaf.exists(),
        "the budget must count removals, not descent"
    );
    assert!(
        old.exists(),
        "the first run must stop at its removal budget"
    );
    assert_eq!(
        resolver
            .remove_deferred_bounded(
                &deferred,
                &CancellationToken::new(),
                usize::MAX,
                Duration::MAX,
            )
            .unwrap(),
        DeferredRemoval::Removed
    );
    assert!(!old.exists());
}

#[cfg(unix)]
#[test]
fn copy_tracks_changes_through_source_symlinks() {
    let (_temp, root, sources, state) = setup();
    let target = sources.join("target");
    let source = sources.join("source-link");
    fs::write(&target, "first").unwrap();
    symlink(&target, &source).unwrap();
    let files = || vec![desired("copy", &source, CopyMode::Copy)];

    run(input(&root, &state, "same", files())).unwrap();
    fs::write(&target, "second").unwrap();
    let counts = run(input(&root, &state, "same", files())).unwrap();

    assert_eq!(counts.updated, 1);
    assert_eq!(fs::read_to_string(root.join("copy")).unwrap(), "second");
}

#[cfg(unix)]
#[test]
fn copy_tracks_a_source_symlink_retargeted_between_store_paths() {
    let mut store_files = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .filter(|path| path.starts_with("/nix/store"))
        .flat_map(|path| fs::read_dir(path).into_iter().flatten().flatten())
        .filter_map(|entry| fs::canonicalize(entry.path()).ok())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    store_files.sort();
    store_files.dedup();
    let Some(first) = store_files.first().cloned() else {
        return;
    };
    let Some(second) = store_files.into_iter().skip(1).find(|path| {
        fs::metadata(path).ok().map(|m| m.len()) != fs::metadata(&first).ok().map(|m| m.len())
    }) else {
        return;
    };

    let (_temp, root, sources, state) = setup();
    let source = sources.join("source-link");
    symlink(&first, &source).unwrap();
    let files = || vec![desired("copy", &source, CopyMode::Copy)];
    run(input(&root, &state, "same", files())).unwrap();

    fs::remove_file(&source).unwrap();
    symlink(&second, &source).unwrap();
    let counts = run(input(&root, &state, "same", files())).unwrap();

    assert_eq!(counts.updated, 1);
    assert_eq!(
        fs::read(root.join("copy")).unwrap(),
        fs::read(second).unwrap()
    );
}

#[test]
fn copy_does_not_trust_a_lexical_store_prefix() {
    if !Path::new("/nix/store").is_dir() {
        return;
    }
    let (_temp, root, sources, state) = setup();
    let target = sources.join("target");
    fs::write(&target, "first").unwrap();
    let disguised = Path::new("/nix/store/../..").join(target.strip_prefix("/").unwrap());
    let files = || vec![desired("copy", &disguised, CopyMode::Copy)];

    run(input(&root, &state, "same", files())).unwrap();
    fs::write(&target, "second").unwrap();
    let counts = run(input(&root, &state, "same", files())).unwrap();

    assert_eq!(counts.updated, 1);
    assert_eq!(fs::read_to_string(root.join("copy")).unwrap(), "second");
}

#[test]
fn copy_rejects_a_destination_inside_its_source_tree() {
    let (_temp, root, _sources, state) = setup();
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("source/value"), "value").unwrap();

    let error = run(input(
        &root,
        &state,
        "self-copy",
        vec![desired(
            "source/generated",
            &root.join("source"),
            CopyMode::Copy,
        )],
    ))
    .unwrap_err();

    assert!(format!("{error:#}").contains("into itself"));
    assert_eq!(
        fs::read_to_string(root.join("source/value")).unwrap(),
        "value"
    );
    assert!(!root.join("source/generated").exists());
    assert!(fs::read_dir(root.join("source")).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".devenv-files-artifact-")
    }));
}

#[cfg(unix)]
#[test]
fn cleanup_removes_only_the_exact_managed_target() {
    let (_temp, root, sources, state) = setup();
    let managed_source = sources.join("managed");
    let replacement = sources.join("replacement");
    fs::write(&managed_source, "managed").unwrap();
    fs::write(&replacement, "replacement").unwrap();

    run(input(
        &root,
        &state,
        "one",
        vec![desired("obsolete", &managed_source, CopyMode::Symlink)],
    ))
    .unwrap();
    let counts = run(input(&root, &state, "empty", vec![])).unwrap();
    assert_eq!(counts.removed, 1);
    assert!(!root.join("obsolete").exists());

    run(input(
        &root,
        &state,
        "two",
        vec![desired("obsolete", &managed_source, CopyMode::Symlink)],
    ))
    .unwrap();
    fs::remove_file(root.join("obsolete")).unwrap();
    symlink(&replacement, root.join("obsolete")).unwrap();

    let (counts, warnings) =
        capture_warnings(|| run(input(&root, &state, "empty-again", vec![])).unwrap());
    assert_eq!(counts.retained, 1);
    assert!(
        warnings.contains("retaining managed file removed from configuration"),
        "{warnings}"
    );
    assert!(warnings.contains("obsolete"), "{warnings}");
    assert_eq!(fs::read_link(root.join("obsolete")).unwrap(), replacement);
    assert!(read_state(&state).unwrap().managed_files.is_empty());

    let counts = run(input(&root, &state, "empty-again", vec![])).unwrap();
    assert_eq!(counts.retained, 0);
    assert_eq!(fs::read_link(root.join("obsolete")).unwrap(), replacement);
}

#[cfg(unix)]
#[test]
fn empty_desired_cleans_legacy_store_symlink_state() {
    let (_temp, root, _sources, state) = setup();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    symlink("/nix/store/aaaaaaaa-legacy", root.join("legacy")).unwrap();
    fs::write(
        &state,
        r#"{"managedFiles":["legacy"],"desiredDigest":"old"}"#,
    )
    .unwrap();

    let counts = run(input(&root, &state, "empty", vec![])).unwrap();
    assert_eq!(counts.removed, 1);
    assert!(fs::symlink_metadata(root.join("legacy")).is_err());
    assert!(read_state(&state).unwrap().managed_files.is_empty());
}

#[test]
fn migrates_absolute_legacy_paths_and_forgets_unsafe_entries() {
    for version in [0, 1] {
        let (_temp, root, _sources, state) = setup();
        fs::create_dir_all(root.join(".claude")).unwrap();
        let source = Path::new("/nix/store/legacy-settings");
        symlink(source, root.join(".claude/settings.json")).unwrap();
        symlink(source, root.join("removed")).unwrap();
        let outside = root.with_file_name("project-other");
        fs::create_dir(&outside).unwrap();
        symlink(source, outside.join("sentinel")).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        let canonical_root = fs::canonicalize(&root).unwrap();
        let old_paths = vec![
            root.join(".claude/settings.json")
                .to_str()
                .unwrap()
                .to_owned(),
            canonical_root
                .join(".claude/settings.json")
                .to_str()
                .unwrap()
                .to_owned(),
            "./.claude/settings.json".to_owned(),
            root.join("removed").to_str().unwrap().to_owned(),
            outside.join("sentinel").to_str().unwrap().to_owned(),
            "../project-other/sentinel".to_owned(),
            root.join("../project-other/sentinel")
                .to_str()
                .unwrap()
                .to_owned(),
            "escape/sentinel".to_owned(),
        ];
        write_test_state(
            &state,
            &FilesState {
                version,
                managed_files: old_paths,
                ..FilesState::default()
            },
        );
        let desired = || {
            input(
                &root,
                &state,
                "current",
                vec![desired(".claude/settings.json", source, CopyMode::Symlink)],
            )
        };
        run(desired()).unwrap();
        let migrated = fs::read(&state).unwrap();
        let saved = read_state(&state).unwrap();
        assert_eq!(saved.managed_files, vec![".claude/settings.json"]);
        assert!(!root.join("removed").is_symlink());
        assert_eq!(fs::read_link(outside.join("sentinel")).unwrap(), source);
        assert_eq!(
            fs::read_link(root.join(".claude/settings.json")).unwrap(),
            source
        );
        run(desired()).unwrap();
        assert_eq!(
            fs::read(&state).unwrap(),
            migrated,
            "migration must converge"
        );
    }
}

#[test]
fn copy_replaces_special_destinations_without_opening_them() {
    use std::os::unix::net::UnixListener;

    let (_temp, root, sources, state) = setup();
    fs::create_dir(&root).unwrap();
    let source = sources.join("regular");
    fs::write(&source, "replacement").unwrap();
    nix::unistd::mkfifo(&root.join("pipe"), nix::sys::stat::Mode::S_IRUSR).unwrap();
    let _socket = UnixListener::bind(root.join("socket")).unwrap();
    // Opening a FIFO must also remain nonblocking if it appears between
    // metadata inspection and open during legacy content comparison.
    let mut resolver = Resolver::new(&root).unwrap();
    let pipe = resolver.destination("pipe", false).unwrap().open().unwrap();
    assert!(!trees_equal(&source, &pipe, &CancellationToken::new()).unwrap());
    run(input(
        &root,
        &state,
        "specials",
        vec![
            desired("pipe", &source, CopyMode::Copy),
            desired("socket", &source, CopyMode::Copy),
        ],
    ))
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("pipe")).unwrap(),
        "replacement"
    );
    assert_eq!(
        fs::read_to_string(root.join("socket")).unwrap(),
        "replacement"
    );
}

#[test]
fn rejects_path_traversal() {
    let (temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "data").unwrap();

    let error = run(input(
        &root,
        &state,
        "bad",
        vec![desired("../outside", &source, CopyMode::Copy)],
    ))
    .unwrap_err();
    assert!(format!("{error:#}").contains("clean relative path"));
    assert!(!temp.path().join("outside").exists());
}

#[test]
fn rejects_parent_child_paths_before_mutation() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "data").unwrap();

    let error = run(input(
        &root,
        &state,
        "bad",
        vec![
            desired("parent", &source, CopyMode::Copy),
            desired("parent/child", &source, CopyMode::Copy),
        ],
    ))
    .unwrap_err();

    assert!(format!("{error:#}").contains("managed file paths overlap"));
    assert!(!root.exists());
}

#[test]
fn rejects_paths_overlapping_state_and_lock_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let sources = temp.path().join("sources");
    let state = root.join(".devenv/state/files.json");
    fs::create_dir(&sources).unwrap();
    let source = sources.join("source");
    fs::write(&source, "data").unwrap();

    for path in [
        ".devenv",
        ".devenv/state",
        ".devenv/state/files.json",
        ".devenv/state/files.json/child",
        ".devenv/state/files.lock",
        ".devenv/state/files.lock/child",
        ".devenv/state/files.recovery",
        ".devenv/state/files.recovery/child",
    ] {
        let error = run(input(
            &root,
            &state,
            "bad",
            vec![desired(path, &source, CopyMode::Copy)],
        ))
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("overlaps internal files state"),
            "unexpected error for {path}: {error:#}"
        );
    }
    assert!(!root.exists());
}

#[cfg(unix)]
#[test]
fn rejects_state_overlap_through_a_root_alias() {
    let temp = tempfile::tempdir().unwrap();
    let actual_root = temp.path().join("actual-project");
    let root_alias = temp.path().join("project");
    let source = temp.path().join("source");
    fs::create_dir(&actual_root).unwrap();
    symlink(&actual_root, &root_alias).unwrap();
    fs::write(&source, "data").unwrap();
    let state = actual_root.join("internal/files.json");

    let error = run(input(
        &root_alias,
        &state,
        "bad",
        vec![desired("internal/files.json", &source, CopyMode::Copy)],
    ))
    .unwrap_err();

    assert!(format!("{error:#}").contains("overlaps internal files state"));
    assert!(!state.exists());
}

#[test]
fn rejects_internal_state_path_aliases() {
    let (_temp, root, sources, mut state) = setup();
    let source = sources.join("source");
    fs::write(&source, "data").unwrap();
    state.set_extension("lock");

    let error = run(input(
        &root,
        &state,
        "bad",
        vec![desired("file", &source, CopyMode::Copy)],
    ))
    .unwrap_err();

    assert!(format!("{error:#}").contains("paths must be distinct"));
    assert!(!root.join("file").exists());
}

#[test]
fn rebuilds_malformed_reconciliation_state() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "data").unwrap();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(&state, "not json").unwrap();

    let counts = run(input(
        &root,
        &state,
        "recovered",
        vec![desired("file", &source, CopyMode::Copy)],
    ))
    .unwrap();

    assert_eq!(counts.created, 1);
    assert_eq!(read_state(&state).unwrap().managed_files, vec!["file"]);
}

#[cfg(unix)]
#[test]
fn permits_distinct_hard_link_names() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "managed").unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("first"), "user-owned").unwrap();
    fs::hard_link(root.join("first"), root.join("second")).unwrap();

    let counts = run(input(
        &root,
        &state,
        "hard-links",
        vec![
            desired("first", &source, CopyMode::Copy),
            desired("second", &source, CopyMode::Copy),
        ],
    ))
    .unwrap();

    assert_eq!(counts.updated, 2);
    assert_eq!(fs::read_to_string(root.join("first")).unwrap(), "managed");
    assert_eq!(fs::read_to_string(root.join("second")).unwrap(), "managed");
}

#[cfg(unix)]
#[test]
fn detects_case_folded_names_when_the_filesystem_uses_them() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "managed").unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("CaseFoldProbe"), "user-owned").unwrap();

    let original_metadata = fs::symlink_metadata(root.join("CaseFoldProbe")).unwrap();
    let case_folding = fs::symlink_metadata(root.join("casefoldprobe")).is_ok_and(|alias| {
        (original_metadata.dev(), original_metadata.ino()) == (alias.dev(), alias.ino())
    });
    if let Some(expected) = std::env::var_os("DEVENV_FILES_EXPECT_CASE_FOLDING") {
        assert_eq!(
            case_folding,
            expected == "1",
            "filesystem case-folding behavior did not match the requested matrix"
        );
    }
    if !case_folding {
        return;
    }

    let error = run(input(
        &root,
        &state,
        "aliases",
        vec![
            desired("CaseFoldProbe", &source, CopyMode::Copy),
            desired("casefoldprobe", &source, CopyMode::Copy),
        ],
    ))
    .unwrap_err();
    assert!(format!("{error:#}").contains("filesystem-equivalent"));
}

#[cfg(unix)]
#[test]
fn detects_new_case_folded_aliases_and_keeps_state_coherent() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "managed").unwrap();
    fs::create_dir_all(&root).unwrap();

    fs::write(root.join("CaseSensitivityProbe"), "probe").unwrap();
    let case_folding = fs::symlink_metadata(root.join("casesensitivityprobe")).is_ok();
    fs::remove_file(root.join("CaseSensitivityProbe")).unwrap();
    if !case_folding {
        return;
    }

    let error = run(input(
        &root,
        &state,
        "aliases",
        vec![
            desired("NewManagedName", &source, CopyMode::Copy),
            desired("newmanagedname", &source, CopyMode::Copy),
        ],
    ))
    .unwrap_err();
    assert!(format!("{error:#}").contains("filesystem-equivalent"));
    assert_eq!(
        read_state(&state).unwrap().managed_files,
        vec!["NewManagedName"]
    );
    assert_eq!(
        fs::read_to_string(root.join("NewManagedName")).unwrap(),
        "managed"
    );
}

#[cfg(unix)]
#[test]
fn unchanged_reconciliation_does_not_replace_state() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    fs::write(&source, "data").unwrap();
    let files = || vec![desired("managed", &source, CopyMode::Symlink)];

    run(input(&root, &state, "same", files())).unwrap();
    let first = fs::metadata(&state).unwrap();
    run(input(&root, &state, "same", files())).unwrap();
    let second = fs::metadata(&state).unwrap();

    assert_eq!(first.dev(), second.dev());
    assert_eq!(first.ino(), second.ino());
}

#[tokio::test]
async fn execute_does_not_publish_diagnostic_counts_as_task_output() {
    let (_temp, root, _sources, state) = setup();
    let output = execute_v1(
        serde_json::to_value(input(&root, &state, "empty", vec![])).unwrap(),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert!(output.0.is_none());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_ancestor_without_touching_its_target() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    let outside = sources.join("outside");
    fs::write(&source, "data").unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.join("linked")).unwrap();

    let error = run(input(
        &root,
        &state,
        "bad",
        vec![desired("linked/file", &source, CopyMode::Copy)],
    ))
    .unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(diagnostic.contains("parent directory is a symlink"));
    assert!(diagnostic.contains("replacing symlinked parents with real directories"));
    assert!(!outside.join("file").exists());
}

#[cfg(unix)]
#[test]
fn cancellation_preserves_unvisited_state() {
    let (_temp, root, sources, state) = setup();
    let first = sources.join("first");
    let second = sources.join("second");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    let files = vec![
        desired("first", &first, CopyMode::Symlink),
        desired("second", &second, CopyMode::Symlink),
    ];
    run(input(&root, &state, "one", files)).unwrap();

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = reconcile(input(&root, &state, "empty", vec![]), cancellation).unwrap_err();
    assert!(format!("{error:#}").contains("cancelled"));
    assert_eq!(
        read_state(&state).unwrap().managed_files,
        vec!["first", "second"]
    );
    assert!(fs::symlink_metadata(root.join("first")).is_ok());
    assert!(fs::symlink_metadata(root.join("second")).is_ok());
}

#[cfg(unix)]
#[test]
fn partial_success_is_recorded_when_a_later_entry_fails() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("source");
    let outside = sources.join("outside");
    fs::write(&source, "data").unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.join("z-linked")).unwrap();

    let error = run(input(
        &root,
        &state,
        "partial",
        vec![
            desired("a-created", &source, CopyMode::Copy),
            desired("z-linked/rejected", &source, CopyMode::Copy),
        ],
    ))
    .unwrap_err();
    assert!(format!("{error:#}").contains("parent directory is a symlink"));
    assert_eq!(fs::read_to_string(root.join("a-created")).unwrap(), "data");
    assert_eq!(read_state(&state).unwrap().managed_files, vec!["a-created"]);
    assert!(!outside.join("rejected").exists());
}

#[test]
fn malformed_recovery_fails_before_destination_mutation() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("new");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(root.join("target"), "old").unwrap();
    fs::write(&source, "new").unwrap();
    let id = Uuid::new_v4();
    fs::write(
        state.with_extension("recovery"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "artifacts": [{
                "id": id.to_string(),
                "intent": {
                    "relative_parent": "",
                    "parent_device": 0,
                    "parent_inode": 0,
                    "name": "not-the-journaled-uuid"
                },
                "phase": "staging"
            }],
            "fallbacks": []
        }))
        .unwrap(),
    )
    .unwrap();

    let error = run(input(
        &root,
        &state,
        "new",
        vec![desired("target", &source, CopyMode::Copy)],
    ))
    .unwrap_err();

    assert!(format!("{error:#}").contains("name does not match its id"));
    assert_eq!(fs::read_to_string(root.join("target")).unwrap(), "old");
}

#[test]
fn recovery_validation_rejects_cross_parent_fallbacks() {
    let id = |parent: &str, phase| {
        let id = Uuid::new_v4().to_string();
        let intent = ArtifactIntent {
            relative_parent: PathBuf::from(parent),
            parent_device: 1,
            parent_inode: 2,
            name: format!(".devenv-files-artifact-{id}"),
        };
        let artifact = Deferred {
            relative_parent: intent.relative_parent.clone(),
            parent_device: intent.parent_device,
            parent_inode: intent.parent_inode,
            name: intent.name.clone(),
            device: 3,
            inode: 4,
        };
        (
            id.clone(),
            RecoveryArtifact {
                id,
                intent,
                artifact: Some(artifact),
                phase,
            },
        )
    };
    let (stage_id, stage) = id("artifacts", ArtifactPhase::Staging);
    let (backup_id, backup) = id("artifacts", ArtifactPhase::Backup);
    let fallback = FallbackRecovery {
        destination: "elsewhere/target".to_string(),
        stage_id,
        backup_id,
        original: EntryIdentity {
            device: 1,
            inode: 2,
            kind: EntryKind::RegularFile,
        },
        staged: EntryIdentity {
            device: 1,
            inode: 3,
            kind: EntryKind::RegularFile,
        },
        placeholder: EntryIdentity {
            device: 1,
            inode: 4,
            kind: EntryKind::RegularFile,
        },
        phase: FallbackPhase::MayMoveOld,
    };
    let journal = RecoveryJournal {
        version: 1,
        artifacts: artifact_map([stage, backup]),
        fallbacks: fallback_map([fallback]),
        ..RecoveryJournal::default()
    };

    let error = validate_recovery_journal(&journal).unwrap_err();
    assert!(format!("{error:#}").contains("destination parent"));
}

#[test]
fn recovery_snapshot_rejects_duplicate_record_ids() {
    let artifact_id = Uuid::new_v4().to_string();
    let artifact = recovery_artifact(artifact_id, ArtifactPhase::Staging);
    let duplicate_artifacts = serde_json::json!({
        "version": 1,
        "artifacts": [artifact.clone(), artifact],
        "fallbacks": []
    });
    let error = serde_json::from_value::<RecoveryJournal>(duplicate_artifacts).unwrap_err();
    assert!(error.to_string().contains("duplicate recovery artifact id"));

    let stage_id = Uuid::new_v4().to_string();
    let fallback = fallback_recovery(
        stage_id,
        Uuid::new_v4().to_string(),
        FallbackPhase::MayMoveOld,
    );
    let duplicate_fallbacks = serde_json::json!({
        "version": 1,
        "artifacts": [],
        "fallbacks": [fallback.clone(), fallback]
    });
    let error = serde_json::from_value::<RecoveryJournal>(duplicate_fallbacks).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("duplicate recovery fallback stage id")
    );
}

#[test]
fn recovery_snapshot_maps_preserve_the_legacy_array_wire_format() {
    #[derive(Deserialize, Serialize)]
    struct LegacyRecoveryJournal {
        version: u32,
        artifacts: Vec<RecoveryArtifact>,
        fallbacks: Vec<FallbackRecovery>,
    }

    let artifact = recovery_artifact(Uuid::new_v4().to_string(), ArtifactPhase::Staging);
    let journal = RecoveryJournal {
        version: 1,
        artifacts: artifact_map([artifact.clone()]),
        ..RecoveryJournal::default()
    };
    let encoded = serde_json::to_value(&journal).unwrap();
    assert!(encoded["artifacts"].is_array());
    assert!(encoded["fallbacks"].is_array());

    let legacy: LegacyRecoveryJournal = serde_json::from_value(encoded).unwrap();
    assert_eq!(legacy.artifacts, vec![artifact.clone()]);
    let decoded: RecoveryJournal =
        serde_json::from_slice(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(decoded.artifacts.get(&artifact.id), Some(&artifact));
    assert!(decoded.fallbacks.is_empty());
}

#[test]
fn recovery_fallback_phase_updates_are_keyed_by_stage_id() {
    let first_stage = Uuid::new_v4().to_string();
    let second_stage = Uuid::new_v4().to_string();
    let first = fallback_recovery(
        first_stage.clone(),
        Uuid::new_v4().to_string(),
        FallbackPhase::MayMoveOld,
    );
    let second = fallback_recovery(
        second_stage.clone(),
        Uuid::new_v4().to_string(),
        FallbackPhase::MayMoveOld,
    );
    let mut journal = RecoveryJournal {
        fallbacks: fallback_map([first, second]),
        ..RecoveryJournal::default()
    };

    journal.fallbacks.get_mut(&first_stage).unwrap().phase = FallbackPhase::MayInstallNew;
    journal.mark_fallback(&first_stage);

    assert_eq!(
        journal.fallbacks[&first_stage].phase,
        FallbackPhase::MayInstallNew
    );
    assert_eq!(
        journal.fallbacks[&second_stage].phase,
        FallbackPhase::MayMoveOld
    );
    assert!(matches!(
        journal.pending.as_slice(),
        [RecoveryEvent::UpsertFallback { fallback }]
            if fallback.stage_id == first_stage
    ));
}

#[test]
fn recovery_event_churn_remains_idempotent_by_id() {
    let mut journal = RecoveryJournal::default();
    let ids = (0..MAX_DEFERRED_CLEANUPS)
        .map(|_| Uuid::new_v4().to_string())
        .collect::<Vec<_>>();
    for (index, id) in ids.iter().enumerate() {
        apply_recovery_event(
            &mut journal,
            RecoveryEvent::UpsertArtifact {
                artifact: recovery_artifact(id.clone(), ArtifactPhase::Staging),
            },
        );
        if index % 2 == 0 {
            apply_recovery_event(
                &mut journal,
                RecoveryEvent::UpsertArtifact {
                    artifact: recovery_artifact(id.clone(), ArtifactPhase::Exchanged),
                },
            );
        }
    }
    for id in ids.iter().step_by(3) {
        apply_recovery_event(
            &mut journal,
            RecoveryEvent::RemoveArtifact { id: id.clone() },
        );
        apply_recovery_event(
            &mut journal,
            RecoveryEvent::RemoveArtifact { id: id.clone() },
        );
    }

    for (index, id) in ids.iter().enumerate() {
        let artifact = journal.artifacts.get(id);
        if index % 3 == 0 {
            assert!(artifact.is_none());
        } else {
            let expected = if index % 2 == 0 {
                ArtifactPhase::Exchanged
            } else {
                ArtifactPhase::Staging
            };
            assert_eq!(artifact.unwrap().phase, expected);
        }
    }
}

#[test]
fn recovery_journal_growth_is_linear_and_tolerates_a_torn_tail() {
    let (_temp, _root, _sources, state) = setup();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    let state_directory = StateDirectory::new(&state).unwrap();
    let mut journal = RecoveryJournal::default();
    for inode in 0..256 {
        let id = Uuid::new_v4().to_string();
        journal.version = 1;
        journal.push_artifact(RecoveryArtifact {
            intent: ArtifactIntent {
                relative_parent: PathBuf::new(),
                parent_device: 1,
                parent_inode: 2,
                name: format!(".devenv-files-artifact-{id}"),
            },
            id,
            artifact: None,
            phase: ArtifactPhase::Staging,
        });
        write_recovery(&state_directory, &mut journal).unwrap();
        assert_eq!(journal.artifacts.len(), inode + 1);
    }
    let recovery_path = state.with_extension("recovery");
    assert!(
        fs::metadata(&recovery_path).unwrap().len() < 256 * 1024,
        "delta journal unexpectedly grew like repeated snapshots"
    );
    let mut recovered = read_recovery_anchored(&state_directory).unwrap();
    assert_eq!(recovered.artifacts.len(), 256);
    let id = Uuid::new_v4().to_string();
    recovered.push_artifact(RecoveryArtifact {
        intent: ArtifactIntent {
            relative_parent: PathBuf::new(),
            parent_device: 1,
            parent_inode: 2,
            name: format!(".devenv-files-artifact-{id}"),
        },
        id,
        artifact: None,
        phase: ArtifactPhase::Staging,
    });
    write_recovery(&state_directory, &mut recovered).unwrap();
    let recovered = read_recovery_anchored(&state_directory).unwrap();
    assert_eq!(recovered.artifacts.len(), 257);

    use std::fs::OpenOptions;
    let mut file = OpenOptions::new()
        .append(true)
        .open(&recovery_path)
        .unwrap();
    file.write_all(b"{\"operation\":\"remove_artifact\",\"id\":\"")
        .unwrap();
    file.write_all(&[0xc3]).unwrap();
    drop(file);
    let mut recovered = read_recovery_anchored(&state_directory).unwrap();
    assert_eq!(recovered.artifacts.len(), 257);
    let id = Uuid::new_v4().to_string();
    recovered.push_artifact(RecoveryArtifact {
        intent: ArtifactIntent {
            relative_parent: PathBuf::new(),
            parent_device: 1,
            parent_inode: 2,
            name: format!(".devenv-files-artifact-{id}"),
        },
        id,
        artifact: None,
        phase: ArtifactPhase::Staging,
    });
    write_recovery(&state_directory, &mut recovered).unwrap();
    assert_eq!(
        read_recovery_anchored(&state_directory)
            .unwrap()
            .artifacts
            .len(),
        258
    );
}

#[test]
fn recovery_adopts_an_intent_only_artifact_into_bounded_cleanup() {
    let (_temp, root, _sources, state) = setup();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    let mut resolver = Resolver::new(&root).unwrap();
    let destination = resolver.destination("target", false).unwrap();
    let id = Uuid::new_v4().to_string();
    let name = format!(".devenv-files-artifact-{id}");
    let intent = destination.artifact_intent(name.clone()).unwrap();
    let state_directory = StateDirectory::new(&state).unwrap();
    let record = RecoveryArtifact {
        id,
        intent,
        artifact: None,
        phase: ArtifactPhase::Staging,
    };
    let mut journal = RecoveryJournal {
        version: 1,
        artifacts: artifact_map([record]),
        ..RecoveryJournal::default()
    };
    write_recovery(&state_directory, &mut journal).unwrap();

    // This is the crash window after mkdir and before its identity is
    // added to the journal.
    drop(
        destination
            .create_artifact(std::ffi::OsStr::new(&name))
            .unwrap(),
    );
    run(input(&root, &state, "empty", Vec::new())).unwrap();

    assert!(!root.join(name).exists());
    assert!(read_state(&state).unwrap().garbage.is_empty());
    assert!(!state.with_extension("recovery").exists());
}

#[test]
fn unresolved_fallback_fails_closed_before_reconciliation() {
    let (_temp, root, sources, state) = setup();
    let staged_source = sources.join("staged");
    let desired_source = sources.join("desired");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(root.join("target"), "old").unwrap();
    fs::write(&staged_source, "staged").unwrap();
    fs::write(&desired_source, "desired").unwrap();
    let mut resolver = Resolver::new(&root).unwrap();
    let destination = resolver.destination("target", false).unwrap();
    let original = destination.metadata().unwrap().unwrap();

    let stage_id = Uuid::new_v4().to_string();
    let stage_name = format!(".devenv-files-artifact-{stage_id}");
    let stage_intent = destination.artifact_intent(stage_name).unwrap();
    let stage_artifact = destination
        .create_artifact(std::ffi::OsStr::new(&stage_intent.name))
        .unwrap();
    let (staged, _file) = destination
        .stage_file(stage_artifact, &staged_source, &CancellationToken::new())
        .unwrap();
    let staged_identity = staged.identity();
    let stage_deferred = staged.deferred();

    let backup_id = Uuid::new_v4().to_string();
    let backup_name = format!(".devenv-files-artifact-{backup_id}");
    let backup_intent = destination.artifact_intent(backup_name).unwrap();
    let backup_artifact = destination
        .create_artifact(std::ffi::OsStr::new(&backup_intent.name))
        .unwrap();
    let mut backup = destination
        .stage_placeholder(backup_artifact, original.kind)
        .unwrap();
    let placeholder = backup.identity();
    let backup_deferred = backup.deferred();
    destination.rename_to(&mut backup).unwrap();

    let mut journal = RecoveryJournal {
        version: 1,
        artifacts: artifact_map([
            RecoveryArtifact {
                id: stage_id.clone(),
                intent: stage_intent,
                artifact: Some(stage_deferred),
                phase: ArtifactPhase::Staging,
            },
            RecoveryArtifact {
                id: backup_id.clone(),
                intent: backup_intent,
                artifact: Some(backup_deferred),
                phase: ArtifactPhase::Backup,
            },
        ]),
        fallbacks: fallback_map([FallbackRecovery {
            destination: "target".to_string(),
            stage_id: stage_id.clone(),
            backup_id: backup_id.clone(),
            original: entry_identity(original),
            staged: entry_identity(staged_identity),
            placeholder: entry_identity(placeholder),
            phase: FallbackPhase::MayInstallNew,
        }]),
        ..RecoveryJournal::default()
    };
    staged.remove(&CancellationToken::new()).unwrap();
    drop(backup);
    write_recovery(&StateDirectory::new(&state).unwrap(), &mut journal).unwrap();

    let error = run(input(
        &root,
        &state,
        "desired",
        vec![desired("target", &desired_source, CopyMode::Copy)],
    ))
    .unwrap_err();

    let rendered = format!("{error:#}");
    assert!(rendered.contains("cannot safely recover"));
    assert!(rendered.contains(&state.with_extension("recovery").display().to_string()));
    assert!(rendered.contains("Only after"));
    assert!(!root.join("target").exists());
    assert!(
        root.join(&journal.artifacts[&backup_id].intent.name)
            .exists()
    );
}

#[cfg(unix)]
fn assert_fallback_recovery(phase: FallbackPhase, move_old: bool) {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("new");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(root.join("target"), "old").unwrap();
    fs::write(&source, "new").unwrap();
    let mut resolver = Resolver::new(&root).unwrap();
    let destination = resolver.destination("target", false).unwrap();
    let original = destination.metadata().unwrap().unwrap();

    let stage_id = Uuid::new_v4().to_string();
    let stage_intent = destination
        .artifact_intent(format!(".devenv-files-artifact-{stage_id}"))
        .unwrap();
    let stage_artifact = destination
        .create_artifact(std::ffi::OsStr::new(&stage_intent.name))
        .unwrap();
    let (staged, _file) = destination
        .stage_file(stage_artifact, &source, &CancellationToken::new())
        .unwrap();
    let stage_identity = staged.identity();
    let stage_deferred = staged.deferred();

    let backup_id = Uuid::new_v4().to_string();
    let backup_intent = destination
        .artifact_intent(format!(".devenv-files-artifact-{backup_id}"))
        .unwrap();
    let backup_artifact = destination
        .create_artifact(std::ffi::OsStr::new(&backup_intent.name))
        .unwrap();
    let mut backup = destination
        .stage_placeholder(backup_artifact, original.kind)
        .unwrap();
    let placeholder = backup.identity();
    let backup_deferred = backup.deferred();

    match phase {
        FallbackPhase::MayMoveOld if move_old => destination.rename_to(&mut backup).unwrap(),
        FallbackPhase::MayInstallNew => {
            destination.rename_to(&mut backup).unwrap();
            destination.rename_from(&staged).unwrap();
        }
        FallbackPhase::MayMoveOld => {}
    }
    drop(staged);
    drop(backup);

    let mut journal = RecoveryJournal {
        version: 1,
        artifacts: artifact_map([
            RecoveryArtifact {
                id: stage_id.clone(),
                intent: stage_intent,
                artifact: Some(stage_deferred.clone()),
                phase: ArtifactPhase::Staging,
            },
            RecoveryArtifact {
                id: backup_id.clone(),
                intent: backup_intent,
                artifact: Some(backup_deferred.clone()),
                phase: ArtifactPhase::Backup,
            },
        ]),
        fallbacks: fallback_map([FallbackRecovery {
            destination: "target".to_string(),
            stage_id: stage_id.clone(),
            backup_id: backup_id.clone(),
            original: entry_identity(original),
            staged: entry_identity(stage_identity),
            placeholder: entry_identity(placeholder),
            phase,
        }]),
        ..RecoveryJournal::default()
    };
    validate_recovery_journal(&journal).unwrap();
    let recovered = recover_artifacts(
        &StateDirectory::new(&state).unwrap(),
        &mut resolver,
        &mut journal,
        &CancellationToken::new(),
    )
    .unwrap();

    let expected = if phase == FallbackPhase::MayInstallNew {
        "new"
    } else {
        "old"
    };
    assert_eq!(fs::read_to_string(root.join("target")).unwrap(), expected);
    assert_eq!(recovered.records.len(), 2);
    for (_, deferred) in recovered.records {
        assert_eq!(
            resolver
                .remove_deferred_bounded(
                    &deferred,
                    &CancellationToken::new(),
                    MAX_DEFERRED_CLEANUP_STEPS_PER_RECORD,
                    MAX_DEFERRED_CLEANUP_DURATION_PER_RUN,
                )
                .unwrap(),
            DeferredRemoval::Removed
        );
    }
    assert!(!root.join(&stage_deferred.name).exists());
    assert!(!root.join(&backup_deferred.name).exists());
    assert_eq!(journal.artifacts.len(), 2);
    assert!(journal.fallbacks.is_empty());
}

#[cfg(unix)]
#[test]
fn recovery_handles_fallback_before_old_move() {
    assert_fallback_recovery(FallbackPhase::MayMoveOld, false);
}

#[cfg(unix)]
#[test]
fn recovery_rolls_back_fallback_after_old_move() {
    assert_fallback_recovery(FallbackPhase::MayMoveOld, true);
}

#[cfg(unix)]
#[test]
fn recovery_finishes_fallback_after_new_install() {
    assert_fallback_recovery(FallbackPhase::MayInstallNew, true);
}

#[test]
fn copies_hidden_ignored_non_utf8_and_repeated_symlink_targets() {
    let (_temp, root, sources, state) = setup();
    let tree = sources.join("tree");
    fs::create_dir_all(tree.join(".hidden")).unwrap();
    fs::write(tree.join(".ignore"), "ignored\n").unwrap();
    fs::write(tree.join("ignored"), "included").unwrap();
    // Darwin filesystems reject invalid UTF-8 at creation; Linux permits it.
    #[cfg(target_os = "linux")]
    let name = <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(b"non-utf8-\xff");
    #[cfg(target_os = "macos")]
    let name = std::ffi::OsStr::new("unicode-λ");
    fs::write(tree.join(".hidden").join(name), "bytes").unwrap();
    symlink(".hidden", tree.join("alias-a")).unwrap();
    symlink(".hidden", tree.join("alias-b")).unwrap();
    let files = || vec![desired("tree", &tree, CopyMode::Copy)];
    assert_eq!(run(input(&root, &state, "v1", files())).unwrap().created, 1);
    assert_eq!(fs::read(root.join("tree/ignored")).unwrap(), b"included");
    for parent in [".hidden", "alias-a", "alias-b"] {
        assert_eq!(
            fs::read(root.join("tree").join(parent).join(name)).unwrap(),
            b"bytes"
        );
    }
    assert_eq!(
        run(input(&root, &state, "v1", files())).unwrap().unchanged,
        1
    );
    fs::write(tree.join(".hidden").join(name), "edit").unwrap();
    assert_eq!(run(input(&root, &state, "v1", files())).unwrap().updated, 1);
}

#[test]
fn invalid_source_trees_preserve_existing_destination_and_recover() {
    for invalid in ["cycle", "dangling", "fifo", "socket"] {
        let (_temp, root, sources, state) = setup();
        let tree = sources.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("value"), "original").unwrap();
        let files = || vec![desired("tree", &tree, CopyMode::Copy)];
        run(input(&root, &state, "v1", files())).unwrap();
        let bad = tree.join("bad");
        let _socket = match invalid {
            "cycle" => {
                symlink(".", &bad).unwrap();
                None
            }
            "dangling" => {
                symlink("missing", &bad).unwrap();
                None
            }
            "fifo" => {
                nix::unistd::mkfifo(&bad, nix::sys::stat::Mode::S_IRUSR).unwrap();
                None
            }
            _ => Some(std::os::unix::net::UnixListener::bind(&bad).unwrap()),
        };
        fs::write(tree.join("value"), "updated").unwrap();
        assert!(
            run(input(&root, &state, "v2", files())).is_err(),
            "{invalid}"
        );
        assert_eq!(
            fs::read(root.join("tree/value")).unwrap(),
            b"original",
            "{invalid}"
        );
        fs::remove_file(&bad).unwrap();
        run(input(&root, &state, "v2", files())).unwrap();
        assert_eq!(fs::read(root.join("tree/value")).unwrap(), b"updated");
    }
}

#[test]
#[ignore = "manual staged fingerprint benchmark"]
fn benchmark_staged_fingerprint_reuse() {
    let (_temp, root, sources, state) = setup();
    let source = sources.join("tree");
    fs::create_dir(&source).unwrap();
    for i in 0..4096 {
        fs::write(source.join(format!("file-{i:04}")), "data").unwrap();
    }
    run(input(
        &root,
        &state,
        "v1",
        vec![desired("tree", &source, CopyMode::Copy)],
    ))
    .unwrap();
    let mut resolver = Resolver::new(&root).unwrap();
    let destination = resolver.destination("tree", false).unwrap();
    let token = CancellationToken::new();
    let staged = destination.fingerprint(&token).unwrap();
    for fresh_walk in [false, true] {
        let start = Instant::now();
        for _ in 0..10 {
            let fingerprint = if fresh_walk {
                destination.fingerprint(&token).unwrap()
            } else {
                fingerprint_after_commit(&destination, staged.clone()).unwrap()
            };
            assert_eq!(fingerprint, staged);
        }
        eprintln!(
            "4096 entries fresh_walk={fresh_walk}: {:?} mean",
            start.elapsed() / 10
        );
    }
}

#[test]
fn same_size_edits_are_detected_in_files_and_trees() {
    for directory in [false, true] {
        let (_temp, root, sources, state) = setup();
        let source = sources.join("source");
        let (source_file, target_file) = if directory {
            fs::create_dir(&source).unwrap();
            (source.join("value"), root.join("target/value"))
        } else {
            (source.clone(), root.join("target"))
        };
        fs::write(&source_file, "one").unwrap();
        let files = |mode| vec![desired("target", &source, mode)];
        run(input(&root, &state, "same", files(CopyMode::Copy))).unwrap();
        fs::write(&source_file, "two").unwrap();
        assert_eq!(
            run(input(&root, &state, "same", files(CopyMode::Copy)))
                .unwrap()
                .updated,
            1
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"two");
        fs::write(&target_file, "own").unwrap();
        assert_eq!(
            run(input(&root, &state, "same", files(CopyMode::Copy)))
                .unwrap()
                .updated,
            1
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"two");
        fs::write(&target_file, "own").unwrap();
        assert_eq!(
            run(input(&root, &state, "link", files(CopyMode::Symlink)))
                .unwrap()
                .conflicting,
            1
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"own");
    }
}
