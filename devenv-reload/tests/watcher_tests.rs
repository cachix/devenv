use devenv_reload::FileWatcher;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test]
async fn test_watcher_detects_file_modification() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("test.nix");

    // Create initial file
    File::create(&file_path)
        .expect("create file")
        .write_all(b"initial content")
        .expect("write");

    // Create watcher
    let mut watcher = FileWatcher::new(&[file_path.clone()]).expect("create watcher");

    // Give watcher time to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Modify file
    File::create(&file_path)
        .expect("open file")
        .write_all(b"modified content")
        .expect("write");

    // Wait for event
    let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

    match event {
        Ok(Some(e)) => assert_eq!(e.path, file_path),
        Ok(None) => panic!("watcher channel closed"),
        Err(_) => panic!("timeout waiting for file change event"),
    }
}

#[tokio::test]
async fn test_watcher_multiple_files() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file1 = temp_dir.path().join("file1.nix");
    let file2 = temp_dir.path().join("file2.nix");

    // Create files
    File::create(&file1)
        .expect("create")
        .write_all(b"1")
        .expect("write");
    File::create(&file2)
        .expect("create")
        .write_all(b"2")
        .expect("write");

    let mut watcher = FileWatcher::new(&[file1.clone(), file2.clone()]).expect("create watcher");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Modify first file
    File::create(&file1)
        .expect("open")
        .write_all(b"1 modified")
        .expect("write");

    let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv())
        .await
        .expect("timeout")
        .expect("event");

    // Should be one of our watched files
    assert!(event.path == file1 || event.path == file2);
}

#[tokio::test]
async fn test_watcher_nonexistent_path_error() {
    let result = FileWatcher::new(&[PathBuf::from("/this/path/does/not/exist/file.nix")]);

    assert!(result.is_err());
}

#[tokio::test]
async fn test_watcher_rapid_modifications() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("rapid.nix");

    File::create(&file_path)
        .expect("create")
        .write_all(b"0")
        .expect("write");

    let mut watcher = FileWatcher::new(&[file_path.clone()]).expect("create watcher");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Rapid modifications
    for i in 1..=5 {
        File::create(&file_path)
            .expect("open")
            .write_all(format!("{}", i).as_bytes())
            .expect("write");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Should receive at least one event
    let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

    assert!(event.is_ok());
}

#[tokio::test]
async fn test_watcher_drops_cleanly() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("drop_test.nix");

    File::create(&file_path)
        .expect("create")
        .write_all(b"test")
        .expect("write");

    {
        let _watcher = FileWatcher::new(&[file_path.clone()]).expect("create watcher");
        // Watcher drops here
    }

    // Should not panic or hang
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_watcher_detects_file_creation_in_watched_dir() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let watch_dir = temp_dir.path().to_path_buf();

    // Watch the directory
    let mut watcher = FileWatcher::new(&[watch_dir.clone()]).expect("create watcher");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create new file in watched directory
    let new_file = watch_dir.join("new_file.nix");
    File::create(&new_file)
        .expect("create file")
        .write_all(b"new content")
        .expect("write");

    let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

    // Should receive an event (either for the file or the directory)
    assert!(event.is_ok());
}

#[tokio::test]
async fn test_watcher_handle_adds_path_at_runtime() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let initial_file = temp_dir.path().join("initial.nix");
    let runtime_file = temp_dir.path().join("runtime.nix");

    // Create initial file
    File::create(&initial_file)
        .expect("create file")
        .write_all(b"initial")
        .expect("write");

    // Create file we'll watch later
    File::create(&runtime_file)
        .expect("create file")
        .write_all(b"runtime")
        .expect("write");

    // Create watcher with only initial file
    let mut watcher = FileWatcher::new(&[initial_file.clone()]).expect("create watcher");
    let handle = watcher.handle();

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Add runtime file via handle
    handle.watch(&runtime_file).expect("add runtime watch");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Modify the runtime file
    File::create(&runtime_file)
        .expect("open file")
        .write_all(b"runtime modified")
        .expect("write");

    // Should receive event for runtime file
    let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv())
        .await
        .expect("timeout")
        .expect("event");

    assert_eq!(event.path, runtime_file);
}
