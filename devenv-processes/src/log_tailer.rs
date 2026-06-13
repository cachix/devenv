use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use devenv_activity::ActivityRef;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// Tail `path` from `start_offset`, invoking `on_line` per complete line
/// (CR/LF stripped). Handles truncation (seek to 0) and replacement (reopen,
/// inode change). Exits when `on_line` returns false, `cancel` fires, or the
/// file disappears after having been opened. When `wait_for_create` is true,
/// a missing file is awaited instead of aborting (the process may not have
/// started yet).
async fn tail_lines<F>(
    path: PathBuf,
    start_offset: u64,
    wait_for_create: bool,
    cancel: CancellationToken,
    mut on_line: F,
) where
    F: FnMut(String) -> bool + Send + 'static,
{
    let file = loop {
        match tokio::fs::File::open(&path).await {
            Ok(f) => break f,
            Err(e) => {
                if !wait_for_create {
                    debug!("failed to open log file {}: {}", path.display(), e);
                    return;
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    _ = cancel.cancelled() => return,
                }
            }
        }
    };

    let (mut ino, len) = match file.metadata().await {
        Ok(m) => (m.ino(), m.len()),
        Err(_) => (0, 0),
    };
    let mut reader = BufReader::new(file);
    if start_offset > 0
        && reader
            .seek(std::io::SeekFrom::Start(start_offset.min(len)))
            .await
            .is_err()
    {
        return;
    }

    // Bytes of a line still missing its newline are kept across EOF waits so
    // a line captured mid-write is emitted whole, never split or duplicated.
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => {
                // EOF — check for truncation or replacement before sleeping
                let position = reader.stream_position().await.unwrap_or(0);

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    _ = cancel.cancelled() => return,
                }

                let meta = match tokio::fs::metadata(&path).await {
                    Ok(m) => m,
                    Err(_) => return,
                };

                if meta.ino() != ino {
                    // File was replaced (e.g., process restart created a new file).
                    // Re-open from the beginning.
                    let file = match tokio::fs::File::open(&path).await {
                        Ok(f) => f,
                        Err(_) => return,
                    };
                    ino = meta.ino();
                    reader = BufReader::new(file);
                    buf.clear();
                } else if meta.len() < position {
                    // Same file but truncated — seek back to the start.
                    if reader.seek(std::io::SeekFrom::Start(0)).await.is_err() {
                        return;
                    }
                    buf.clear();
                }
                // Otherwise the file just hasn't grown yet; loop will re-read.
            }
            Ok(_) => {
                if buf.last() != Some(&b'\n') {
                    // Partial line at EOF; keep the bytes until the writer
                    // finishes the line.
                    continue;
                }
                // Decode lossily: a single non-UTF8 byte must not kill the
                // tail, matching read_tail's semantics.
                let mut line = String::from_utf8_lossy(&buf).into_owned();
                buf.clear();
                // Drop CR/LF. Under a pty the OS translates "\n" writes to
                // "\r\n", so log files commonly have CRLF line endings; any
                // bare "\r" surviving into the TUI moves the terminal cursor
                // back to column 0 mid-row and erases rendered content.
                // Progress-style writers also emit mid-line "\r" between
                // updates, which get bundled into one read_line record.
                line.retain(|c| c != '\r' && c != '\n');
                if !on_line(line) {
                    return;
                }
            }
            Err(e) => {
                debug!("error reading log file {}: {}", path.display(), e);
                return;
            }
        }
    }
}

/// Spawn a task that tails a log file and emits lines to an activity.
///
/// The file is already created/truncated by start_command before the job
/// starts, so a missing file aborts instead of being awaited. Lifecycle is
/// managed by aborting the returned handle.
pub fn spawn_file_tailer(path: PathBuf, activity: ActivityRef, is_stderr: bool) -> JoinHandle<()> {
    spawn_tail_to(path, 0, false, CancellationToken::new(), move |line| {
        if is_stderr {
            activity.error(&line);
        } else {
            activity.log(&line);
        }
        true
    })
}

/// Tail from `start_offset` into a caller-supplied sink; used by attach
/// streams. The tail stops when `on_line` returns false or `cancel` fires.
pub fn spawn_tail_to<F>(
    path: PathBuf,
    start_offset: u64,
    wait_for_create: bool,
    cancel: CancellationToken,
    on_line: F,
) -> JoinHandle<()>
where
    F: FnMut(String) -> bool + Send + 'static,
{
    tokio::spawn(tail_lines(
        path,
        start_offset,
        wait_for_create,
        cancel,
        on_line,
    ))
}

/// Last `max_lines` complete lines of `path` (CR stripped, at most 64 KiB
/// scanned, decoded lossily) and the byte offset just past the last newline —
/// the offset a follow-up tail must start from so a partial trailing line is
/// emitted whole. Missing or unreadable file yields `(vec![], 0)`.
pub fn read_backlog(path: &Path, max_lines: usize) -> (Vec<String>, u64) {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return (Vec::new(), 0);
    };
    let Ok(metadata) = file.metadata() else {
        return (Vec::new(), 0);
    };

    let file_size = metadata.len();
    let read_size = file_size.min(64 * 1024);
    let start_pos = file_size - read_size;
    if file.seek(SeekFrom::Start(start_pos)).is_err() {
        return (Vec::new(), 0);
    }
    let mut bytes = Vec::with_capacity(read_size as usize);
    if file.read_to_end(&mut bytes).is_err() {
        return (Vec::new(), 0);
    }

    let Some(last_nl) = bytes.iter().rposition(|&b| b == b'\n') else {
        // No complete line in the window; the follow-up tail starts at the
        // window so an oversized line is at least picked up from there.
        return (Vec::new(), start_pos);
    };
    let offset = start_pos + last_nl as u64 + 1;

    let mut parts: Vec<&[u8]> = bytes[..=last_nl].split(|&b| b == b'\n').collect();
    // The slice ends with the final newline, so the last split element is
    // always empty.
    parts.pop();
    // A window that does not start at byte 0 may begin mid-line; that first
    // fragment is not a complete line.
    let skip_partial_head = if start_pos > 0 { 1 } else { 0 };
    let take_from = parts
        .len()
        .saturating_sub(max_lines)
        .max(skip_partial_head.min(parts.len()));
    let lines = parts[take_from..]
        .iter()
        .map(|raw| {
            let mut line = String::from_utf8_lossy(raw).into_owned();
            line.retain(|c| c != '\r');
            line
        })
        .collect();
    (lines, offset)
}
