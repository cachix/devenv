use avt::Vt;
use devenv_shell::{
    CommandBuilder, PtySize, SessionConfig, SessionIo, ShellCommand, ShellEvent, ShellSession,
};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

/// Create a SessionIo wired to UnixStream pairs for testing.
/// Returns (io, stdin_write_end, stdout_read_end).
fn test_io() -> (SessionIo, UnixStream, UnixStream) {
    let (stdin_ours, stdin_theirs) = UnixStream::pair().unwrap();
    let (stdout_theirs, stdout_ours) = UnixStream::pair().unwrap();
    let io = SessionIo {
        stdin: Some(Box::new(stdin_theirs)),
        stdout: Some(Box::new(stdout_theirs)),
    };
    (io, stdin_ours, stdout_ours)
}

fn test_session() -> ShellSession {
    ShellSession::new(SessionConfig {
        show_status_line: false,
        size: Some(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }),
    })
}

fn spawn_cmd(shell_line: &str) -> ShellCommand {
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg(shell_line);
    ShellCommand::Spawn {
        command: cmd,
        watch_files: vec![],
    }
}

/// Read from a UnixStream until `needle` is found or deadline expires.
/// Returns all bytes read so far.
fn read_until(stream: &mut UnixStream, needle: &[u8], deadline: Duration) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let start = std::time::Instant::now();
    let mut buf = [0u8; 4096];
    let mut collected = Vec::new();
    while start.elapsed() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                collected.extend_from_slice(&buf[..n]);
                if collected.windows(needle.len()).any(|w| w == needle) {
                    return collected;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        }
    }
    collected
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_spawn_and_exit() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, mut event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("exit 0")).await.unwrap();

    // Wait for Exited event
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(ShellEvent::Exited { .. }) => break,
                    None => panic!("event channel closed without Exited"),
                    _ => continue,
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                panic!("timed out waiting for Exited event");
            }
        }
    }

    let result = handle.await.unwrap();
    assert!(result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_shutdown_before_spawn() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(ShellCommand::Shutdown).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("timed out")
        .unwrap();
    assert!(result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pty_output_to_stdout() {
    let (io, _stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx
        .send(spawn_cmd("echo MARKER_OUTPUT; exit 0"))
        .await
        .unwrap();

    let collected = read_until(&mut stdout_ours, b"MARKER_OUTPUT", Duration::from_secs(5));
    assert!(
        collected
            .windows(b"MARKER_OUTPUT".len())
            .any(|w| w == b"MARKER_OUTPUT"),
        "expected MARKER_OUTPUT in stdout, got: {:?}",
        String::from_utf8_lossy(&collected)
    );

    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stdin_forwarded_to_pty() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    // Spawn head -1 which reads one line, prints it, and exits
    cmd_tx.send(spawn_cmd("head -1")).await.unwrap();

    // Write to stdin — gets forwarded to PTY, head -1 echoes it and exits
    stdin_ours.write_all(b"HELLO_STDIN\n").unwrap();
    stdin_ours.flush().unwrap();

    let collected = read_until(&mut stdout_ours, b"HELLO_STDIN", Duration::from_secs(5));
    assert!(
        collected
            .windows(b"HELLO_STDIN".len())
            .any(|w| w == b"HELLO_STDIN"),
        "expected HELLO_STDIN in stdout, got: {:?}",
        String::from_utf8_lossy(&collected)
    );

    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_ctrl_alt_d_toggle_pause() {
    let (io, mut stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, mut event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    // Send Ctrl-Alt-D (ESC + Ctrl-D)
    stdin_ours.write_all(&[0x1b, 0x04]).unwrap();
    stdin_ours.flush().unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(ShellEvent::TogglePause) => break,
                    None => panic!("event channel closed without TogglePause"),
                    _ => continue,
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                panic!("timed out waiting for TogglePause event");
            }
        }
    }

    // Unblock "read unused" so the PTY process exits
    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}

fn status_line_session() -> ShellSession {
    ShellSession::new(SessionConfig {
        show_status_line: true,
        size: Some(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }),
    })
}

/// Render captured stdout through a virtual terminal and return visible viewport row texts.
fn render(stdout_bytes: &[u8], cols: usize, rows: usize) -> Vec<String> {
    let mut vt = Vt::new(cols, rows);
    vt.feed_str(&String::from_utf8_lossy(stdout_bytes));
    vt.view()
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect()
}

/// Insta filters to normalize timing and spinner in status line snapshots.
fn status_line_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (r"[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]", "[SPIN]"),
        (r" for \d+(m \d+|\.\d+)?(ms|s)", " for [TIME]"),
        (r" in \d+(m \d+|\.\d+)?(ms|s)", " in [TIME]"),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_status_line_rendered_on_last_row() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    // Tell the session about watched files — this triggers the "watching" status
    cmd_tx
        .send(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into(), "b.nix".into()],
        })
        .await
        .unwrap();

    // Wait for the status line to appear in stdout
    let collected = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));
    let rows = render(&collected, 80, 24);

    // Only snapshot the last row (status line) — upper rows are empty/irrelevant
    insta::assert_snapshot!(rows[23]);

    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_scroll_region_preserves_status_line() {
    let (io, _stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    // Spawn a shell that floods 30 lines (more than the 23-row scroll region),
    // then prints a marker and exits
    cmd_tx
        .send(spawn_cmd(
            "for i in $(seq 1 30); do echo \"line$i\"; done; echo DONE; exit 0",
        ))
        .await
        .unwrap();

    // Tell the session about watched files so the status line has visible content
    cmd_tx
        .send(ShellCommand::WatchedFiles {
            files: vec!["test.nix".into()],
        })
        .await
        .unwrap();

    // Wait for all output to arrive
    let collected = read_until(&mut stdout_ours, b"DONE", Duration::from_secs(5));
    let rows = render(&collected, 80, 24);

    // Snapshot the full viewport — shell output should be in rows 0-22,
    // status line should be on row 23 (protected by scroll region)
    insta::assert_snapshot!(rows.join("\n"));

    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_build_lifecycle_status_line() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into(), "b.nix".into()],
        })
        .await
        .unwrap();
    let mut all_bytes = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));

    // Building state
    cmd_tx
        .send(ShellCommand::Building {
            changed_files: vec![PathBuf::from("devenv.nix")],
        })
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"building",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::with_settings!({ filters => status_line_filters() }, {
        insta::assert_snapshot!("building", rows[23]);
    });

    // Reload ready state
    cmd_tx
        .send(ShellCommand::ReloadReady {
            changed_files: vec![PathBuf::from("devenv.nix")],
        })
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"ready",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::with_settings!({ filters => status_line_filters() }, {
        insta::assert_snapshot!("reload_ready", rows[23]);
    });

    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_build_failed_error_toggle() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into()],
        })
        .await
        .unwrap();
    let mut all_bytes = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));

    // Build failed
    cmd_tx
        .send(ShellCommand::BuildFailed {
            changed_files: vec![PathBuf::from("devenv.nix")],
            error: "attribute 'foo' missing".to_string(),
        })
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"failed",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::with_settings!({ filters => status_line_filters() }, {
        insta::assert_snapshot!("failed_status", rows[23]);
    });

    // Ctrl-Alt-E to show error
    stdin_ours.write_all(&[0x1b, 0x05]).unwrap();
    stdin_ours.flush().unwrap();

    // Wait for "hide error" — this appears in the status line redraw AFTER the error text,
    // ensuring we capture both the error output and the updated status line
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"hide error",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    // Snapshot the viewport showing the error text and updated status line
    insta::with_settings!({ filters => status_line_filters() }, {
        insta::assert_snapshot!("error_displayed", rows.join("\n"));
    });

    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_watching_paused_status_line() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into()],
        })
        .await
        .unwrap();
    let mut all_bytes = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));

    // Pause watching
    cmd_tx
        .send(ShellCommand::WatchingPaused { paused: true })
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"paused",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::assert_snapshot!("paused", rows[23]);

    // Resume watching
    cmd_tx
        .send(ShellCommand::WatchingPaused { paused: false })
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"watching",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::assert_snapshot!("resumed", rows[23]);

    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_print_watched_files() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, None, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(ShellCommand::PrintWatchedFiles {
            files: vec![
                PathBuf::from("devenv.nix"),
                PathBuf::from("devenv.yaml"),
                PathBuf::from("shell.nix"),
            ],
        })
        .await
        .unwrap();

    let collected = read_until(&mut stdout_ours, b"shell.nix", Duration::from_secs(5));
    let rows = render(&collected, 80, 24);

    // Snapshot the visible rows that contain file listing
    let non_empty: Vec<_> = rows.iter().filter(|r| !r.is_empty()).collect();
    insta::assert_snapshot!(
        non_empty
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );

    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}
