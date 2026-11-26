//! Terminal utilities.
//!
//! Provides utilities for managing terminal state, including raw mode handling.

#[cfg(unix)]
use crate::escape::parse_default_color_osc;
use libghostty_vt::style::RgbColor;
use std::io::{self, IsTerminal, Write};

/// Check if stdin is a TTY.
pub fn is_tty() -> bool {
    io::stdin().is_terminal()
}

#[derive(Default)]
pub(crate) struct TerminalProbe {
    pub cursor_row: Option<u16>,
    /// Default foreground, background, and cursor colors, in OSC order.
    pub colors: [Option<RgbColor>; 3],
    /// Input read during takeover that belongs to the child, including typeahead.
    pub input: Vec<u8>,
}

pub(crate) fn probe_terminal(stdout: &mut impl Write) -> TerminalProbe {
    #[cfg(unix)]
    {
        probe_terminal_fd(&io::stdin(), stdout, std::time::Duration::from_secs(2))
    }
    #[cfg(not(unix))]
    {
        let _ = stdout;
        TerminalProbe {
            cursor_row: crossterm::cursor::position().ok().map(|(_, row)| row + 1),
            ..TerminalProbe::default()
        }
    }
}

#[cfg(unix)]
fn probe_terminal_fd(
    stdin: &impl std::os::fd::AsFd,
    stdout: &mut impl Write,
    timeout: std::time::Duration,
) -> TerminalProbe {
    use nix::errno::Errno;
    use nix::poll::{PollFd, PollFlags, poll};
    use std::time::Instant;

    let mut probe = TerminalProbe::default();
    // CPR is the final reply. It lets us finish immediately on terminals
    // that don't answer some or all of the color queries.
    let requests = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]12;?\x1b\\\x1b[6n";
    if let Err(error) = stdout.write_all(requests).and_then(|()| stdout.flush()) {
        tracing::debug!(%error, "could not send terminal takeover probe");
        return probe;
    }

    let deadline = Instant::now() + timeout;
    let mut descriptors = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
    let mut buffer = [0; 1024];
    let mut pending = Vec::new();
    while probe.cursor_row.is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let millis = u16::try_from(remaining.as_millis().max(1)).unwrap_or(u16::MAX);
        let ready = match poll(&mut descriptors, millis) {
            Ok(ready) => ready,
            Err(Errno::EINTR) => continue,
            Err(error) => {
                tracing::debug!(%error, "terminal takeover poll failed");
                break;
            }
        };
        if ready == 0 {
            continue;
        }
        let len = match nix::unistd::read(stdin, &mut buffer) {
            Ok(0) => break,
            Ok(len) => len,
            Err(Errno::EINTR | Errno::EAGAIN) => continue,
            Err(error) => {
                tracing::debug!(%error, "terminal takeover read failed");
                break;
            }
        };
        pending.extend_from_slice(&buffer[..len]);
        probe.collect_replies(&mut pending);
    }
    probe.input.extend_from_slice(&pending);
    probe
}

#[cfg(unix)]
impl TerminalProbe {
    fn collect_replies(&mut self, pending: &mut Vec<u8>) {
        // Only the tail can contain an incomplete report. Retain enough bytes
        // for the full RGB or cursor report while preserving longer typeahead.
        const MAX_REPORT_LEN: usize = 128;
        let mut offset = 0;
        for (start, _) in pending
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == 0x1b)
        {
            if start < offset {
                continue;
            }
            let data = &pending[start..];
            let end = if let Some((color, rest)) = parse_default_color_osc(data) {
                let slot = &mut self.colors[usize::from(color.code - 10)];
                *slot = std::str::from_utf8(color.value)
                    .ok()
                    .and_then(|value| RgbColor::parse(value).ok());
                pending.len() - rest.len()
            } else if self.cursor_row.is_none()
                && let Some((row, len)) = parse_cursor_report(data)
            {
                self.cursor_row = Some(row);
                start + len
            } else {
                continue;
            };
            self.input.extend_from_slice(&pending[offset..start]);
            offset = end;
        }
        let keep_from = offset.max(pending.len().saturating_sub(MAX_REPORT_LEN));
        self.input.extend_from_slice(&pending[offset..keep_from]);
        pending.drain(..keep_from);
    }
}

#[cfg(unix)]
fn parse_cursor_report(input: &[u8]) -> Option<(u16, usize)> {
    let body = input.strip_prefix(b"\x1b[")?;
    let end = body
        .iter()
        .position(|byte| !byte.is_ascii_digit() && *byte != b';')?;
    if body[end] != b'R' {
        return None;
    }
    let (row, col) = std::str::from_utf8(&body[..end]).ok()?.split_once(';')?;
    let row: u16 = row.parse().ok()?;
    let col: u16 = col.parse().ok()?;
    (row > 0 && col > 0).then_some((row, end + 3))
}

/// Raw terminal mode guard that restores terminal state on drop.
///
/// When created, this guard puts the terminal into raw mode. When dropped,
/// it restores the original terminal settings. This ensures proper cleanup
/// even if the program panics or exits early.
pub struct RawModeGuard {
    #[cfg(unix)]
    original: Option<libc::termios>,
}

impl RawModeGuard {
    /// Enter raw mode. Returns a guard that restores settings on drop.
    ///
    /// If stdin is not a TTY (e.g., in CI or tests), this is a no-op.
    pub fn new() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = io::stdin().as_raw_fd();

            // Skip raw mode if stdin is not a terminal (e.g., in CI or tests)
            if unsafe { libc::isatty(fd) } == 0 {
                return Ok(Self { original: None });
            }
            // A background process group would be stopped by tcsetattr.
            if unsafe { libc::tcgetpgrp(fd) } != unsafe { libc::getpgrp() } {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "stdin is not the foreground terminal",
                ));
            }

            let mut termios: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(fd, &mut termios) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let original = termios;

            unsafe { libc::cfmakeraw(&mut termios) };
            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self {
                original: Some(original),
            })
        }

        #[cfg(not(unix))]
        Ok(Self {})
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(original) = self.original {
            use std::os::unix::io::AsRawFd;
            let fd = io::stdin().as_raw_fd();
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tty_returns_bool() {
        // Just verify it doesn't panic and returns a bool
        let _ = is_tty();
    }

    #[test]
    fn test_raw_mode_guard_creation() {
        // In test environment, this should succeed even if not a TTY
        let guard = RawModeGuard::new();
        assert!(guard.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn takeover_probe_collects_colors_and_preserves_typeahead() {
        let (stdin, mut terminal) = io::pipe().unwrap();
        terminal
            .write_all(
                b"before\x1b]10;rgb:12/34/56\x1b\\\x1b]11;rgb:abcd/1234/0000\x07\x1b[7;2Rafter",
            )
            .unwrap();
        let mut stdout = Vec::new();
        let probe = probe_terminal_fd(&stdin, &mut stdout, std::time::Duration::from_secs(1));
        assert_eq!(
            stdout,
            b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]12;?\x1b\\\x1b[6n"
        );
        assert_eq!(probe.cursor_row, Some(7));
        assert_eq!(
            probe.colors,
            [
                Some(RgbColor {
                    r: 0x12,
                    g: 0x34,
                    b: 0x56
                }),
                Some(RgbColor {
                    r: 0xab,
                    g: 0x12,
                    b: 0
                }),
                None,
            ]
        );
        assert_eq!(probe.input, b"beforeafter");
    }

    #[cfg(unix)]
    #[test]
    fn takeover_probe_handles_fragmented_replies() {
        let mut probe = TerminalProbe::default();
        let mut pending = b"\x1b]11;rgb:1212/3434/".to_vec();
        probe.collect_replies(&mut pending);
        assert_eq!(probe.colors[1], None);
        pending.extend_from_slice(b"5656\x1b");
        probe.collect_replies(&mut pending);
        assert_eq!(probe.colors[1], None);
        pending.extend_from_slice(b"\\\x1b[7;");
        probe.collect_replies(&mut pending);
        assert_eq!(
            probe.colors[1],
            Some(RgbColor {
                r: 0x12,
                g: 0x34,
                b: 0x56
            })
        );
        assert_eq!(probe.cursor_row, None);
        pending.extend_from_slice(b"2Rkey");
        probe.collect_replies(&mut pending);
        assert_eq!(probe.cursor_row, Some(7));
        assert_eq!(pending, b"key");
    }
}
