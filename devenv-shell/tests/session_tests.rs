#![cfg(feature = "test-pty")]

use devenv_mailbox::{FrontendCommand, FrontendEvent, ProcessCommand};
use devenv_shell::keybindings::{ShellAction, ShellKeyChord, ShellKeyCode, ShellKeybindings};
use devenv_shell::vt_utils::{DEFAULT_MAX_SCROLLBACK, active_point, row_plain_text, screen_point};
use devenv_shell::{
    CommandBuilder, PtySize, SessionConfig, SessionIo, ShellCommand, ShellEvent, ShellSession,
};
use libghostty_vt::terminal::Terminal;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
        ..SessionConfig::default()
    })
}

fn spawn_cmd(shell_line: &str) -> FrontendCommand {
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg(shell_line);
    FrontendCommand::Shell(ShellCommand::Spawn {
        command: cmd,
        watch_files: vec![],
    })
}

fn shell(command: ShellCommand) -> FrontendCommand {
    FrontendCommand::Shell(command)
}

/// Floods 30 numbered lines followed by a DONE marker, overflowing a 24-row terminal.
const FLOOD_CMD: &str = "for i in $(seq 1 30); do echo \"line$i\"; done; echo DONE; exit 0";

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
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("exit 0")).await.unwrap();

    // Wait for Exited event
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(FrontendEvent::Shell(ShellEvent::Exited { .. })) => break,
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
async fn test_exit_event_waits_for_mailbox_capacity() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(1);
    let (event_tx, mut event_rx) = mpsc::channel(1);

    event_tx
        .send(FrontendEvent::Process(ProcessCommand::StopManager))
        .await
        .unwrap();

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });
    cmd_tx.send(spawn_cmd("exit 7")).await.unwrap();

    assert!(matches!(
        event_rx.recv().await,
        Some(FrontendEvent::Process(ProcessCommand::StopManager))
    ));
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
            .await
            .expect("timed out waiting for shell exit event"),
        Some(FrontendEvent::Shell(ShellEvent::Exited {
            exit_code: Some(7)
        }))
    ));

    assert_eq!(handle.await.unwrap().unwrap(), Some(7));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_shutdown_before_spawn() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(shell(ShellCommand::Shutdown)).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("timed out")
        .unwrap();
    assert!(result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cancellation_before_spawn_does_not_wait_for_mailbox_close() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (_cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);
    let shutdown = CancellationToken::new();

    let session = test_session().with_shutdown_token(shutdown.clone());
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    shutdown.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("session ignored cancellation while waiting for Spawn")
        .unwrap()
        .expect("session returned an error during cancellation");
    assert_eq!(result, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_frontend_command_before_spawn_is_rejected() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(FrontendCommand::ExitRenderer).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("timed out")
        .unwrap();
    assert!(matches!(
        result,
        Err(devenv_shell::SessionError::UnexpectedCommand(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_shutdown_after_spawn() {
    let (io, _stdin_ours, _stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();
    cmd_tx.send(shell(ShellCommand::Shutdown)).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("session ignored shutdown after spawning the PTY")
        .unwrap()
        .expect("session returned an error during shutdown");
    assert_eq!(result, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pty_output_to_stdout() {
    let (io, _stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

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
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

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
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, mut event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx
        .send(spawn_cmd("printf 'READY\\n'; read unused"))
        .await
        .unwrap();

    // Wait until the session's stdin reader and PTY event loop are running.
    // A Unix stream has no record boundaries; writing before the reader is
    // blocked made this test's two-byte local keybinding timing-dependent.
    let ready = read_until(&mut stdout_ours, b"READY", Duration::from_secs(5));
    assert!(
        ready
            .windows(b"READY".len())
            .any(|window| window == b"READY"),
        "session did not become ready: {:?}",
        String::from_utf8_lossy(&ready)
    );

    // Send Ctrl-Alt-D (ESC + Ctrl-D)
    stdin_ours.write_all(&[0x1b, 0x04]).unwrap();
    stdin_ours.flush().unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(FrontendEvent::Shell(ShellEvent::TogglePause)) => break,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_custom_shell_keybinding_replaces_default() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let mut keybindings = ShellKeybindings::default();
    let binding = ShellKeyChord::new(ShellKeyCode::Function(12), false, false, false);
    keybindings.replace(ShellAction::TogglePause, vec![binding.clone()]);
    let session = ShellSession::new(SessionConfig {
        show_status_line: false,
        size: Some(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }),
        keybindings,
    });
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx
        .send(spawn_cmd("printf 'READY\\n'; read unused"))
        .await
        .unwrap();
    let ready = read_until(&mut stdout_ours, b"READY", Duration::from_secs(5));
    assert!(ready.windows(5).any(|window| window == b"READY"));

    stdin_ours.write_all(&binding.terminal_bytes()).unwrap();
    stdin_ours.flush().unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        FrontendEvent::Shell(ShellEvent::TogglePause)
    ));

    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_custom_shell_keybinding_is_forwarded_in_alternate_screen() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);
    let mut keybindings = ShellKeybindings::default();
    let binding = ShellKeyChord::new(ShellKeyCode::Function(12), false, false, false);
    keybindings.replace(ShellAction::TogglePause, vec![binding.clone()]);
    let session = ShellSession::new(SessionConfig {
        show_status_line: false,
        size: Some(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }),
        keybindings,
    });
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx
        .send(spawn_cmd(
            "stty raw -echo; printf '\\033[?1049hREADY'; dd bs=1 count=5 2>/dev/null | od -An -t x1; printf 'CAPTURED'; dd bs=1 count=1 2>/dev/null >/dev/null; printf '\\033[?1049l'",
        ))
        .await
        .unwrap();
    let mut collected = read_until(&mut stdout_ours, b"READY", Duration::from_secs(5));
    assert!(
        collected.windows(5).any(|window| window == b"READY"),
        "alternate-screen application did not become ready: {:?}",
        String::from_utf8_lossy(&collected)
    );

    stdin_ours.write_all(&binding.terminal_bytes()).unwrap();
    stdin_ours.flush().unwrap();
    collected.extend(read_until(
        &mut stdout_ours,
        b"CAPTURED",
        Duration::from_secs(5),
    ));
    assert!(
        collected
            .windows(b"1b 5b 32 34 7e".len())
            .any(|window| window == b"1b 5b 32 34 7e"),
        "custom keybinding was not forwarded: {:?}",
        String::from_utf8_lossy(&collected)
    );

    stdin_ours.write_all(b"x").unwrap();
    stdin_ours.flush().unwrap();
    drop(stdin_ours);
    drop(cmd_tx);
    assert_eq!(handle.await.unwrap().unwrap(), Some(0));
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
        ..SessionConfig::default()
    })
}

/// Render captured stdout through a virtual terminal and return visible viewport row texts.
fn render(stdout_bytes: &[u8], cols: usize, rows: usize) -> Vec<String> {
    let mut vt = Terminal::new(cols as u16, rows as u16).unwrap();
    vt.set_scrollback_max_bytes(Some(0)).unwrap();
    vt.vt_write(stdout_bytes);
    (0..rows)
        .map(|y| {
            row_plain_text(&vt, active_point(y as u32))
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// Render captured stdout and return ALL lines (scrollback + viewport).
/// Tests that scrolled-off content was correctly pushed into native scrollback.
fn render_all_lines(stdout_bytes: &[u8], cols: usize, rows: usize) -> Vec<String> {
    let mut vt = Terminal::new(cols as u16, rows as u16).unwrap();
    vt.set_scrollback_max_bytes(Some(DEFAULT_MAX_SCROLLBACK))
        .unwrap();
    vt.vt_write(stdout_bytes);
    let total = vt.total_rows().unwrap_or(0);
    (0..total)
        .map(|y| {
            row_plain_text(&vt, screen_point(y as u32))
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_status_line_rendered_on_last_row() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    // Tell the session about watched files — this triggers the "watching" status
    cmd_tx
        .send(shell(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into(), "b.nix".into()],
        }))
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
async fn test_overflow_preserved_in_scrollback() {
    let (io, _stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd(FLOOD_CMD)).await.unwrap();

    let collected = read_until(&mut stdout_ours, b"DONE", Duration::from_secs(5));

    // All 30 lines must appear in scrollback + viewport — nothing lost
    let all_lines = render_all_lines(&collected, 80, 24);
    let non_empty: Vec<_> = all_lines
        .iter()
        .filter(|r| !r.is_empty())
        .cloned()
        .collect();
    for i in 1..=30 {
        let expected = format!("line{}", i);
        assert!(
            non_empty.iter().any(|l| l.contains(&expected)),
            "expected '{}' in scrollback + viewport, but it was lost",
            expected
        );
    }

    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wrapped_line_preserved_in_scrollback() {
    let (io, _stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    // Print a 100-char single-token line (wraps once at col 80), then push it
    // off the 24-row viewport so it lands in native scrollback.
    let marker = "X".repeat(100);
    let cmd = format!(
        "printf '%s\\n' '{}'; for i in $(seq 1 30); do echo line$i; done; echo DONE; exit 0",
        marker
    );
    cmd_tx.send(spawn_cmd(&cmd)).await.unwrap();

    let collected = read_until(&mut stdout_ours, b"DONE", Duration::from_secs(5));

    let mut vt = Terminal::new(80, 24).unwrap();
    vt.set_scrollback_max_bytes(Some(DEFAULT_MAX_SCROLLBACK))
        .unwrap();
    vt.vt_write(&collected);

    let total = vt.total_rows().unwrap_or(0);
    let mut found_wrapped = false;
    for y in 0..total {
        let point = screen_point(y as u32);
        if !row_plain_text(&vt, point).contains("XXXXXXXXXX") {
            continue;
        }
        let row = vt.grid_ref(point).unwrap().row().unwrap();
        if row.is_wrapped().unwrap_or(false) {
            found_wrapped = true;
            break;
        }
    }
    assert!(
        found_wrapped,
        "expected the wrapped line in scrollback to keep its soft-wrap bit"
    );

    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_overflow_viewport_shows_tail() {
    let (io, _stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = test_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd(FLOOD_CMD)).await.unwrap();

    let collected = read_until(&mut stdout_ours, b"DONE", Duration::from_secs(5));

    let rows = render(&collected, 80, 24);
    let non_empty: Vec<_> = rows.iter().filter(|r| !r.is_empty()).collect();
    insta::assert_snapshot!(
        non_empty
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );

    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_overflow_status_line_protected() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    // Flood 30 lines then keep the process alive so the status line isn't
    // cleared by the PtyExit handler.
    cmd_tx
        .send(spawn_cmd(
            "for i in $(seq 1 30); do echo \"line$i\"; done; echo DONE; read unused",
        ))
        .await
        .unwrap();

    cmd_tx
        .send(shell(ShellCommand::WatchedFiles {
            files: vec!["test.nix".into()],
        }))
        .await
        .unwrap();

    // Wait for both the flood output and the status line render
    let mut collected = read_until(&mut stdout_ours, b"DONE", Duration::from_secs(5));
    if !collected
        .windows(b"watching".len())
        .any(|w| w == b"watching")
    {
        collected.extend(read_until(
            &mut stdout_ours,
            b"watching",
            Duration::from_secs(5),
        ));
    }

    let rows = render(&collected, 80, 24);
    insta::assert_snapshot!(rows[23]);

    // Keep consuming renderer output while the child exits. Stopping at the
    // first DONE/status marker can fill the test socket while the renderer is
    // presenting the remaining flood, preventing it from reading stdin.
    let drain_stdout = tokio::task::spawn_blocking(move || {
        let mut remaining = Vec::new();
        let _ = stdout_ours.read_to_end(&mut remaining);
    });

    // Unblock the process so it can exit.
    let _ = stdin_ours.write_all(b"\n");
    drop(stdin_ours);
    drop(cmd_tx);
    let _ = handle.await;
    let _ = drain_stdout.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_build_lifecycle_status_line() {
    let (io, mut stdin_ours, mut stdout_ours) = test_io();
    let (cmd_tx, cmd_rx) = mpsc::channel(10);
    let (event_tx, _event_rx) = mpsc::channel(10);

    let session = status_line_session();
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(shell(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into(), "b.nix".into()],
        }))
        .await
        .unwrap();
    let mut all_bytes = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));

    // Building state
    cmd_tx
        .send(shell(ShellCommand::Building {
            changed_files: vec![PathBuf::from("devenv.nix")],
        }))
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"building",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::assert_snapshot!("building", &rows[23]);

    // Reload ready state
    cmd_tx
        .send(shell(ShellCommand::ReloadReady {
            changed_files: vec![PathBuf::from("devenv.nix")],
        }))
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"ready",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::assert_snapshot!("reload_ready", &rows[23]);

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
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(shell(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into()],
        }))
        .await
        .unwrap();
    let mut all_bytes = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));

    // Build failed
    cmd_tx
        .send(shell(ShellCommand::BuildFailed {
            changed_files: vec![PathBuf::from("devenv.nix")],
            error: "attribute 'foo' missing".to_string(),
        }))
        .await
        .unwrap();
    all_bytes.extend(read_until(
        &mut stdout_ours,
        b"failed",
        Duration::from_secs(5),
    ));
    let rows = render(&all_bytes, 80, 24);
    insta::assert_snapshot!("failed_status", &rows[23]);

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
    insta::assert_snapshot!("error_displayed", rows.join("\n"));

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
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(shell(ShellCommand::WatchedFiles {
            files: vec!["a.nix".into()],
        }))
        .await
        .unwrap();
    let mut all_bytes = read_until(&mut stdout_ours, b"watching", Duration::from_secs(5));

    // Pause watching
    cmd_tx
        .send(shell(ShellCommand::WatchingPaused { paused: true }))
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
        .send(shell(ShellCommand::WatchingPaused { paused: false }))
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
    let handle = tokio::spawn(async move { session.run(cmd_rx, event_tx, io).await });

    cmd_tx.send(spawn_cmd("read unused")).await.unwrap();

    cmd_tx
        .send(shell(ShellCommand::PrintWatchedFiles {
            files: vec![
                PathBuf::from("devenv.nix"),
                PathBuf::from("devenv.yaml"),
                PathBuf::from("shell.nix"),
            ],
        }))
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
