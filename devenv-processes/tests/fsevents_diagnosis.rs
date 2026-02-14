//! Diagnostic test to narrow down WHY FSEvents doesn't deliver events
//! in the Nix sandbox. Tests each backend independently:
//!
//! 1. notify recommended_watcher (FSEvents on macOS)
//! 2. raw kqueue syscalls (kernel-level, no daemon)
//! 3. notify PollWatcher (mtime-based polling)

use notify::{Event, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Test FSEvents via notify's recommended_watcher (FSEvents on macOS)
#[test]
fn test_fsevents_recommended_watcher() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();
    let watch_file = watch_dir.join("test.txt");
    std::fs::write(&watch_file, "initial").unwrap();

    eprintln!("[FSEvents] watch dir: {watch_dir:?}");
    eprintln!("[FSEvents] canonical: {:?}", watch_dir.canonicalize());

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        eprintln!("[FSEvents] callback: {res:?}");
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .expect("create watcher");

    watcher
        .watch(&watch_dir, RecursiveMode::Recursive)
        .expect("watch");
    eprintln!("[FSEvents] watching {watch_dir:?}");

    std::thread::sleep(Duration::from_millis(500));
    while rx.try_recv().is_ok() {}

    eprintln!("[FSEvents] writing to {watch_file:?}");
    std::fs::write(&watch_file, "modified").unwrap();

    let events = collect_events(&rx, "FSEvents", Duration::from_secs(5));
    if events.is_empty() {
        eprintln!("[FSEvents] no events from write, trying new file...");
        std::fs::write(watch_dir.join("new.txt"), "new").unwrap();
        let events = collect_events(&rx, "FSEvents", Duration::from_secs(5));
        if events.is_empty() {
            eprintln!("[FSEvents] NO EVENTS - FSEvents is not working");
        }
    }
}

/// Test kqueue directly (kernel syscalls, no FSEvents daemon)
#[cfg(target_os = "macos")]
#[test]
fn test_kqueue_direct() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();
    let watch_file = watch_dir.join("test.txt");
    std::fs::write(&watch_file, "initial").unwrap();

    eprintln!("[kqueue] watch file: {watch_file:?}");

    let mut watcher = kqueue::Watcher::new().expect("create kqueue watcher");
    watcher
        .add_filename(
            &watch_file,
            kqueue::EventFilter::EVFILT_VNODE,
            kqueue::FilterFlag::NOTE_WRITE | kqueue::FilterFlag::NOTE_EXTEND,
        )
        .expect("add filename");
    watcher.watch().expect("start watching");

    eprintln!("[kqueue] watching, now writing...");
    std::fs::write(&watch_file, "modified").unwrap();

    match watcher.poll(Some(Duration::from_secs(5))) {
        Some(event) => eprintln!("[kqueue] GOT EVENT: {event:?}"),
        None => eprintln!("[kqueue] NO EVENT within 5s"),
    }

    // Also test watching a directory for new files
    let mut dir_watcher = kqueue::Watcher::new().expect("create kqueue dir watcher");
    dir_watcher
        .add_filename(
            &watch_dir,
            kqueue::EventFilter::EVFILT_VNODE,
            kqueue::FilterFlag::NOTE_WRITE,
        )
        .expect("add dir");
    dir_watcher.watch().expect("start watching dir");

    eprintln!("[kqueue] watching dir, creating new file...");
    std::fs::write(watch_dir.join("new.txt"), "new content").unwrap();

    match dir_watcher.poll(Some(Duration::from_secs(5))) {
        Some(event) => eprintln!("[kqueue] dir GOT EVENT: {event:?}"),
        None => eprintln!("[kqueue] dir NO EVENT within 5s"),
    }
}

/// Test poll watcher (pure filesystem polling, no OS notifications)
#[test]
fn test_poll_watcher() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();
    let watch_file = watch_dir.join("test.txt");
    std::fs::write(&watch_file, "initial").unwrap();

    eprintln!("[PollWatcher] watch dir: {watch_dir:?}");

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::PollWatcher::new(
        move |res: Result<Event, notify::Error>| {
            eprintln!("[PollWatcher] callback: {res:?}");
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        notify::Config::default().with_poll_interval(Duration::from_millis(200)),
    )
    .expect("create poll watcher");

    watcher
        .watch(&watch_dir, RecursiveMode::Recursive)
        .expect("watch");

    std::thread::sleep(Duration::from_millis(500));
    while rx.try_recv().is_ok() {}

    eprintln!("[PollWatcher] writing to {watch_file:?}");
    std::fs::write(&watch_file, "modified").unwrap();

    let events = collect_events(&rx, "PollWatcher", Duration::from_secs(3));
    if events.is_empty() {
        // mtime truncation may hide first write — try again after 1s
        eprintln!("[PollWatcher] no event (mtime truncation?), writing again after 1s...");
        std::thread::sleep(Duration::from_secs(1));
        std::fs::write(&watch_file, "modified again").unwrap();
        let events = collect_events(&rx, "PollWatcher", Duration::from_secs(3));
        if events.is_empty() {
            eprintln!("[PollWatcher] still NO EVENTS");
        }
    }
}

fn collect_events(rx: &mpsc::Receiver<Event>, name: &str, timeout: Duration) -> Vec<Event> {
    let mut events = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => {
                eprintln!("[{name}] GOT EVENT: {event:?}");
                events.push(event);
                std::thread::sleep(Duration::from_millis(200));
                while let Ok(e) = rx.try_recv() {
                    events.push(e);
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    events
}

/// Print environment info
#[test]
fn test_print_env_info() {
    eprintln!("=== Environment Info ===");
    eprintln!("OS: {}", std::env::consts::OS);
    eprintln!("ARCH: {}", std::env::consts::ARCH);

    let tmp = tempfile::tempdir().expect("temp dir");
    eprintln!("temp path: {:?}", tmp.path());
    eprintln!("canonical: {:?}", tmp.path().canonicalize());

    let fsevents_dev = PathBuf::from("/dev/fsevents");
    eprintln!("/dev/fsevents exists: {}", fsevents_dev.exists());
    if let Ok(meta) = std::fs::metadata(&fsevents_dev) {
        eprintln!("/dev/fsevents metadata: {meta:?}");
    } else {
        eprintln!("/dev/fsevents: not accessible");
    }

    for var in ["NIX_BUILD_TOP", "NIX_SANDBOX", "TMPDIR", "HOME"] {
        eprintln!("{}={:?}", var, std::env::var(var).ok());
    }
}
