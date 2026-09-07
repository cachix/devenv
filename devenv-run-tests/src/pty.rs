//! PTY driver for tests that script interactive sessions.
//!
//! Stand-in for util-linux `script -qefc CMD FILE`, which BSD/macOS `script`
//! cannot express (no -f/-c). Stdin is a directive script, one per line:
//!
//! - `expect:TEXT` — wait until TEXT appears in session output. Each match
//!   consumes output up to and including it, so repeated patterns match
//!   successive occurrences.
//! - `send:BYTES` — write BYTES to the PTY, decoding printf-style escapes
//!   (`\003`, `\x03`, `\e`, `\r`, `\n`, `\t`, `\\`).
//! - `resize:COLSxROWS` — resize the PTY and deliver the resulting SIGWINCH
//!   (for example, `resize:80x24`).
//! - `run:CMD` — run CMD via `sh -c`; a non-zero exit fails the session.
//!   The command is bounded by the same timeout as an expectation.

use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix::sys::signal::{Signal, killpg};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// Run `command` on a fresh PTY, driven by the directive script on stdin,
/// capturing PTY output to `transcript`. Mirrors the child's exit status,
/// with 128+N for death by signal N (so a SIGINT-terminated session reports
/// 130, like `script -e`). Each `expect` and the final drain are bounded by
/// `step_timeout`; on expiry the process group is killed and the driver
/// exits 124. A failed `run` directive exits 125. Input is always sent exactly
/// once: retrying an interactive command can hide dropped-input bugs or repeat
/// a non-idempotent action.
pub fn run(transcript: &Path, command: &str, step_timeout: Duration) -> Result<i32> {
    let pty = native_pty_system()
        .openpty(PtySize {
            // The TUI assertions assume a 40x120 terminal.
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| miette!("failed to open pty: {e}"))?;

    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg(command);
    cmd.cwd(std::env::current_dir().into_diagnostic()?);

    let child = pty
        .slave
        .spawn_command(cmd)
        .map_err(|e| miette!("failed to spawn command on pty: {e}"))?;
    // Close our slave handle so the reader sees EOF/EIO once the child's side
    // is gone.
    drop(pty.slave);

    let child_pid = child
        .process_id()
        .ok_or_else(|| miette!("spawned child has no pid"))? as i32;
    let mut child_guard = ChildGroupGuard::new(child_pid);

    // Signals (e.g. CI cancellation) land on us, not the children. Track a
    // controller-side `run:` group as well as the PTY group so neither can be
    // orphaned while the main thread is blocked in a directive.
    let active_run_group = Arc::new(AtomicI32::new(0));
    let signal_run_group = Arc::clone(&active_run_group);
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).into_diagnostic()?;
    thread::spawn(move || {
        if let Some(signum) = signals.forever().next() {
            let run_pid = signal_run_group.load(Ordering::SeqCst);
            if run_pid != 0 {
                kill_group_escalating(run_pid);
            }
            kill_group_escalating(child_pid);
            std::process::exit(128 + signum);
        }
    });

    let writer = pty
        .master
        .take_writer()
        .map_err(|e| miette!("failed to open pty writer: {e}"))?;
    let writer = Arc::new(Mutex::new(writer));
    let mut reader = pty
        .master
        .try_clone_reader()
        .map_err(|e| miette!("failed to open pty reader: {e}"))?;
    let mut out = File::create(transcript)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to create {}", transcript.display()))?;
    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
    let terminal_writer = Arc::clone(&writer);
    let reader_thread = thread::spawn(move || {
        // Crossterm probes for the Kitty keyboard protocol whenever an iocraft
        // event loop starts. A bare PTY has no terminal emulator to answer it,
        // which leaves startup dependent on a timeout and can race scripted input.
        // Reply only to the primary-device-attributes query: this explicitly
        // selects legacy key decoding, matching a terminal without keyboard
        // enhancement support.
        const KEYBOARD_PROBE: &[u8] = b"\x1b[?u\x1b[c";
        const PRIMARY_DEVICE_ATTRIBUTES: &[u8] = b"\x1b[?1;2c";
        const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";
        const CURSOR_POSITION: &[u8] = b"\x1b[6;1R";
        let mut probe_output = Vec::new();
        let mut buf = [0u8; 4096];
        'read: loop {
            // Linux raises EIO once the slave side is closed; macOS returns EOF.
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    probe_output.extend_from_slice(&buf[..n]);
                    while let Some(pos) = find(&probe_output, KEYBOARD_PROBE) {
                        let Ok(mut writer) = terminal_writer.lock() else {
                            break 'read;
                        };
                        if writer.write_all(PRIMARY_DEVICE_ATTRIBUTES).is_err()
                            || writer.flush().is_err()
                        {
                            break 'read;
                        }
                        probe_output.drain(..pos + KEYBOARD_PROBE.len());
                    }
                    while let Some(pos) = find(&probe_output, CURSOR_POSITION_QUERY) {
                        let Ok(mut writer) = terminal_writer.lock() else {
                            break 'read;
                        };
                        if writer.write_all(CURSOR_POSITION).is_err() || writer.flush().is_err() {
                            break 'read;
                        }
                        probe_output.drain(..pos + CURSOR_POSITION_QUERY.len());
                    }
                    let keep = KEYBOARD_PROBE
                        .len()
                        .max(CURSOR_POSITION_QUERY.len())
                        .saturating_sub(1);
                    if probe_output.len() > keep {
                        probe_output.drain(..probe_output.len() - keep);
                    }
                    let _ = chunk_tx.send(buf[..n].to_vec());
                }
            }
        }
    });

    let mut output: Vec<u8> = Vec::new();
    let mut search_pos = 0;
    for line in std::io::stdin().lock().lines() {
        let line = line.into_diagnostic()?;
        if line.is_empty() {
            continue;
        }
        if let Some(pattern) = line.strip_prefix("expect:") {
            let needle = pattern.as_bytes();
            let deadline = Instant::now() + step_timeout;
            loop {
                if let Some(pos) = find(&output[search_pos..], needle) {
                    search_pos += pos + needle.len();
                    break;
                }
                let now = Instant::now();
                if now >= deadline {
                    fail_expect("expect timed out", pattern, &output);
                    child_guard.terminate();
                    return Ok(124);
                }
                match chunk_rx.recv_timeout(deadline - now) {
                    Ok(chunk) => output.extend_from_slice(&chunk),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if find(&output[search_pos..], needle).is_none() {
                            fail_expect(
                                "session output ended before expect matched",
                                pattern,
                                &output,
                            );
                            child_guard.terminate();
                            return Ok(124);
                        }
                    }
                }
            }
        } else if let Some(bytes) = line.strip_prefix("send:") {
            let mut writer = writer
                .lock()
                .map_err(|_| miette!("PTY writer lock poisoned"))?;
            writer.write_all(&decode_send(bytes)?).into_diagnostic()?;
            writer.flush().into_diagnostic()?;
        } else if let Some(size) = line.strip_prefix("resize:") {
            pty.master
                .resize(parse_size(size)?)
                .map_err(|error| miette!("failed to resize PTY to {size}: {error}"))?;
        } else if let Some(cmd) = line.strip_prefix("run:") {
            match run_command(cmd, step_timeout, &active_run_group)? {
                Some(status) if status.success() => {}
                Some(status) => {
                    eprintln!("run failed ({status}): {cmd}");
                    child_guard.terminate();
                    return Ok(125);
                }
                None => {
                    eprintln!("run timed out: {cmd}");
                    child_guard.terminate();
                    return Ok(124);
                }
            }
        } else {
            child_guard.terminate();
            return Err(miette!("unknown directive: {line}"));
        }
    }

    // Directives are done; the child gets `step_timeout` to exit on its own.
    let timed_out = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let deadline_flag = Arc::clone(&timed_out);
    thread::spawn(move || {
        if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(step_timeout) {
            deadline_flag.store(true, Ordering::SeqCst);
            kill_group_escalating(child_pid);
        }
    });

    let status = waitpid(Pid::from_raw(child_pid), None).into_diagnostic()?;
    drop(done_tx);
    // Drain output produced between the last read and child exit.
    let _ = reader_thread.join();
    child_guard.disarm();

    if timed_out.load(Ordering::SeqCst) {
        return Ok(124);
    }
    match status {
        WaitStatus::Exited(_, code) => Ok(code),
        WaitStatus::Signaled(_, sig, _) => Ok(128 + sig as i32),
        other => Err(miette!("unexpected wait status: {other:?}")),
    }
}

fn parse_size(value: &str) -> Result<PtySize> {
    let (cols, rows) = value
        .split_once('x')
        .ok_or_else(|| miette!("invalid PTY size {value:?}; expected COLSxROWS"))?;
    let cols = cols
        .parse::<u16>()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid PTY column count in {value:?}"))?;
    let rows = rows
        .parse::<u16>()
        .into_diagnostic()
        .wrap_err_with(|| format!("invalid PTY row count in {value:?}"))?;
    if cols == 0 || rows == 0 {
        return Err(miette!("PTY dimensions must be greater than zero"));
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}

/// Owns the spawned PTY process group until it has been reaped normally.
/// This covers setup, parsing, and I/O errors that return through `?`.
struct ChildGroupGuard {
    pid: i32,
    armed: bool,
}

impl ChildGroupGuard {
    fn new(pid: i32) -> Self {
        Self { pid, armed: true }
    }

    fn terminate(&mut self) {
        if !self.armed {
            return;
        }
        kill_group_escalating(self.pid);
        let _ = waitpid(Pid::from_raw(self.pid), None);
        self.armed = false;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ChildGroupGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Run a controller-side command in its own process group. `None` means it
/// exceeded the deadline and was terminated; the caller maps that to exit 124.
fn run_command(
    command: &str,
    timeout: Duration,
    active_group: &AtomicI32,
) -> Result<Option<ExitStatus>> {
    let mut child = std::process::Command::new("/bin/sh");
    child.arg("-c").arg(command).process_group(0);
    let mut child = child.spawn().into_diagnostic()?;
    let pid = child.id() as i32;
    active_group.store(pid, Ordering::SeqCst);
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                active_group.store(0, Ordering::SeqCst);
                return Ok(Some(status));
            }
            Ok(None) => {}
            Err(error) => {
                kill_group_escalating(pid);
                let _ = child.wait();
                active_group.store(0, Ordering::SeqCst);
                return Err(error).into_diagnostic();
            }
        }
        if Instant::now() >= deadline {
            kill_group_escalating(pid);
            let _ = child.wait();
            active_group.store(0, Ordering::SeqCst);
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn fail_expect(reason: &str, pattern: &str, output: &[u8]) {
    eprintln!("{reason}: {pattern}");
    let tail = &output[output.len().saturating_sub(800)..];
    eprintln!("session output tail: {:?}", String::from_utf8_lossy(tail));
}

/// SIGTERM the group, escalating to SIGKILL if it survives the grace period.
/// Each child is a process-group leader, so its pid names the group.
fn kill_group_escalating(pid: i32) {
    for sig in [Signal::SIGTERM, Signal::SIGKILL] {
        if killpg(Pid::from_raw(pid), sig).is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Decode printf-style escapes: `\NNN` octal, `\xHH` hex, `\e`, `\r`, `\n`,
/// `\t`, `\\`.
fn decode_send(s: &str) -> Result<Vec<u8>> {
    let b = s.as_bytes();
    let mut decoded = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            decoded.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        match b.get(i) {
            Some(b'\\') => {
                decoded.push(b'\\');
                i += 1;
            }
            Some(b'r') => {
                decoded.push(b'\r');
                i += 1;
            }
            Some(b'n') => {
                decoded.push(b'\n');
                i += 1;
            }
            Some(b't') => {
                decoded.push(b'\t');
                i += 1;
            }
            Some(b'e') => {
                decoded.push(0x1b);
                i += 1;
            }
            Some(b'x') => {
                let hex = s
                    .get(i + 1..i + 3)
                    .ok_or_else(|| miette!("truncated \\x escape in send: {s}"))?;
                decoded.push(
                    u8::from_str_radix(hex, 16)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("bad \\x escape in send: {s}"))?,
                );
                i += 3;
            }
            Some(b'0'..=b'7') => {
                let mut value: u32 = 0;
                let mut digits = 0;
                while digits < 3 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                    value = value * 8 + u32::from(b[i] - b'0');
                    i += 1;
                    digits += 1;
                }
                decoded.push(
                    u8::try_from(value)
                        .map_err(|_| miette!("octal escape out of range in send: {s}"))?,
                );
            }
            _ => return Err(miette!("unknown escape in send: {s}")),
        }
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_octal_hex_and_letters() {
        assert_eq!(decode_send(r"\003").unwrap(), vec![3]);
        assert_eq!(decode_send(r"\x1b[B").unwrap(), vec![0x1b, b'[', b'B']);
        assert_eq!(decode_send(r"\033[B\033[B").unwrap(), b"\x1b[B\x1b[B");
        assert_eq!(decode_send(r"a\r\n\t\\b").unwrap(), b"a\r\n\t\\b");
        assert_eq!(decode_send("s").unwrap(), b"s");
    }

    #[test]
    fn rejects_bad_escapes() {
        assert!(decode_send(r"\q").is_err());
        assert!(decode_send(r"tail\").is_err());
        assert!(decode_send(r"\x0").is_err());
    }

    #[test]
    fn parses_terminal_size_as_columns_by_rows() {
        let size = parse_size("120x40").unwrap();
        assert_eq!(size.cols, 120);
        assert_eq!(size.rows, 40);
        assert!(parse_size("120").is_err());
        assert!(parse_size("0x40").is_err());
        assert!(parse_size("120xnope").is_err());
    }

    #[test]
    fn finds_successive_occurrences() {
        let hay = b"one marker two marker three";
        let first = find(hay, b"marker").unwrap();
        assert_eq!(first, 4);
        assert_eq!(find(&hay[first + 6..], b"marker"), Some(5));
    }

    #[test]
    fn run_commands_report_success_and_failure() {
        let active_group = AtomicI32::new(0);
        assert!(
            run_command("exit 0", Duration::from_secs(1), &active_group)
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(
            !run_command("exit 7", Duration::from_secs(1), &active_group)
                .unwrap()
                .unwrap()
                .success()
        );
        assert_eq!(active_group.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn run_commands_time_out() {
        let active_group = AtomicI32::new(0);
        assert!(
            run_command("sleep 10", Duration::from_millis(20), &active_group)
                .unwrap()
                .is_none()
        );
        assert_eq!(active_group.load(Ordering::SeqCst), 0);
    }
}
