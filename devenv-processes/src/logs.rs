//! Bounded process output capture. File writes and subscription snapshots share
//! a lock, so rotation cannot race history reads or the backlog/live handoff.
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::config::LogRetentionConfig;

const MAX_LINE_BYTES: usize = 64 * 1024;
const HISTORY_BYTES: usize = 1024 * 1024;
const LIVE_LINES: usize = 256;

fn segment(path: &Path, index: usize) -> PathBuf {
    if index == 0 {
        path.to_owned()
    } else {
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".{index}"));
        name.into()
    }
}

struct StreamState {
    path: PathBuf,
    config: LogRetentionConfig,
    file: Option<File>,
    len: u64,
    failed: bool,
    pending: Vec<u8>,
    live: broadcast::Sender<String>,
}

impl StreamState {
    fn warning(&mut self, error: io::Error) {
        if !self.failed {
            tracing::warn!(path = %self.path.display(), %error, "Failed to store process output; continuing live capture");
        }
        self.failed = true;
        self.file = None;
    }

    fn reset(&mut self, config: LogRetentionConfig) {
        self.file = None;
        self.len = 0;
        self.failed = false;
        self.pending.clear();
        self.config = config;
        let result = (|| {
            let parent = self.path.parent().expect("log directory");
            std::fs::create_dir_all(parent)?;
            // Remove all old numbered segments, including those beyond a new,
            // smaller retention count. Do not touch unrelated files.
            let prefix = format!("{}.", self.path.file_name().unwrap().to_string_lossy());
            for entry in std::fs::read_dir(parent)? {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                    })
                {
                    std::fs::remove_file(entry.path())?;
                }
            }
            self.file = Some(
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&self.path)?,
            );
            Ok(())
        })();
        if let Err(error) = result {
            self.warning(error);
        }
    }

    fn write(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        if self.failed {
            return Ok(());
        }
        while !bytes.is_empty() {
            if self.len == self.config.max_file_size {
                self.file = None;
                let oldest = segment(&self.path, self.config.max_files - 1);
                match std::fs::remove_file(oldest) {
                    Ok(()) => (),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e),
                }
                for index in (1..self.config.max_files).rev() {
                    match std::fs::rename(
                        segment(&self.path, index - 1),
                        segment(&self.path, index),
                    ) {
                        Ok(()) => (),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                        Err(e) => return Err(e),
                    }
                }
                self.file = Some(File::create(&self.path)?);
                self.len = 0;
            }
            let count = bytes
                .len()
                .min((self.config.max_file_size - self.len).min(usize::MAX as u64) as usize);
            self.file
                .as_mut()
                .ok_or_else(|| io::Error::other("log file is not open"))?
                .write_all(&bytes[..count])?;
            self.len += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn emit(&mut self, lines: &mut Vec<String>) {
        let mut line = String::from_utf8_lossy(&self.pending).into_owned();
        line.retain(|c| c != '\r' && c != '\n');
        self.pending.clear();
        let _ = self.live.send(line.clone());
        lines.push(line);
    }

    fn capture(&mut self, bytes: &[u8], eof: bool) -> Vec<String> {
        if let Err(error) = self.write(bytes) {
            self.warning(error);
        }
        let mut lines = Vec::new();
        for &byte in bytes {
            self.pending.push(byte);
            // Also bound unfinished lines: a process need not emit newlines.
            if byte == b'\n' || self.pending.len() >= MAX_LINE_BYTES {
                self.emit(&mut lines);
            }
        }
        if eof && !self.pending.is_empty() {
            self.emit(&mut lines);
        }
        lines
    }

    fn history(&self, max_lines: usize, complete_only: bool) -> Vec<String> {
        if max_lines == 0 {
            return Vec::new();
        }
        // Read backwards across files without allocating the full configured
        // retention budget. The window can cross arbitrary segment boundaries.
        let mut chunks = Vec::new();
        let mut remaining = HISTORY_BYTES;
        for index in 0..self.config.max_files {
            if remaining == 0 {
                break;
            }
            let Ok(mut file) = File::open(segment(&self.path, index)) else {
                continue;
            };
            let Ok(meta) = file.metadata() else {
                continue;
            };
            let count = meta.len().min(remaining as u64) as usize;
            if file.seek(SeekFrom::End(-(count as i64))).is_err() {
                continue;
            }
            let mut bytes = vec![0; count];
            if file.read_exact(&mut bytes).is_err() {
                continue;
            }
            remaining -= count;
            chunks.push(bytes);
        }
        let mut bytes: Vec<u8> = chunks.into_iter().rev().flatten().collect();
        if complete_only {
            bytes.truncate(bytes.len().saturating_sub(self.pending.len()));
        }
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_suffix('\n').unwrap_or(&text);
        if text.is_empty() && bytes.is_empty() {
            return Vec::new();
        }
        let start = text
            .rmatch_indices('\n')
            .nth(max_lines - 1)
            .map_or(0, |(index, _)| index + 1);
        text[start..]
            .split('\n')
            .map(|line| line.replace('\r', ""))
            .collect()
    }
}

pub struct LogStreamStore {
    state: Mutex<StreamState>,
}

impl LogStreamStore {
    fn new(path: PathBuf, config: LogRetentionConfig) -> Arc<Self> {
        let (live, _) = broadcast::channel(LIVE_LINES);
        let stream = Arc::new(Self {
            state: Mutex::new(StreamState {
                path,
                config,
                file: None,
                len: 0,
                failed: false,
                pending: Vec::new(),
                live,
            }),
        });
        stream.reset(config);
        stream
    }

    fn reset(&self, config: LogRetentionConfig) {
        self.state.lock().unwrap().reset(config);
    }

    pub fn tail(&self, lines: usize) -> String {
        self.state.lock().unwrap().history(lines, false).join("\n")
    }

    /// Atomically snapshot complete history and subscribe to subsequent lines.
    pub fn subscribe(&self, lines: usize) -> (Vec<String>, broadcast::Receiver<String>) {
        let state = self.state.lock().unwrap();
        let receiver = state.live.subscribe();
        (state.history(lines, true), receiver)
    }
}

pub struct ProcessLogs {
    pub stdout: Arc<LogStreamStore>,
    pub stderr: Arc<LogStreamStore>,
}

impl ProcessLogs {
    pub fn new(state_dir: &Path, name: &str, config: LogRetentionConfig) -> Self {
        let (stdout, stderr) = crate::command::log_paths(state_dir, name);
        Self {
            stdout: LogStreamStore::new(stdout, config),
            stderr: LogStreamStore::new(stderr, config),
        }
    }

    pub fn reset(&self, config: LogRetentionConfig) {
        self.stdout.reset(config);
        self.stderr.reset(config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(size: u64, count: usize) -> (tempfile::TempDir, ProcessLogs) {
        let dir = tempfile::tempdir().unwrap();
        let logs = ProcessLogs::new(
            dir.path(),
            "test",
            LogRetentionConfig {
                max_file_size: size,
                max_files: count,
            },
        );
        (dir, logs)
    }

    fn captured(stream: &LogStreamStore, bytes: &[u8], eof: bool) {
        stream.state.lock().unwrap().capture(bytes, eof);
    }

    fn retained(stream: &LogStreamStore) -> Vec<u8> {
        let state = stream.state.lock().unwrap();
        let mut bytes = Vec::new();
        for index in (0..state.config.max_files).rev() {
            if let Ok(data) = std::fs::read(segment(&state.path, index)) {
                assert!(data.len() as u64 <= state.config.max_file_size);
                bytes.extend(data);
            }
        }
        assert!(bytes.len() as u64 <= state.config.max_file_size * state.config.max_files as u64);
        bytes
    }

    #[test]
    fn oversized_writes_and_single_file_rotation_are_strictly_bounded() {
        for count in [1, 2, 3] {
            let (_dir, logs) = fixture(8, count);
            let bytes: Vec<u8> = (0..101).collect();
            captured(&logs.stdout, &bytes, true);
            let stored = retained(&logs.stdout);
            assert!(bytes.ends_with(&stored));
            assert!(!stored.is_empty());
            assert_eq!(&stored[stored.len() - 5..], &bytes[96..]);
        }
    }

    #[test]
    fn history_joins_lines_split_across_segments() {
        let (_dir, logs) = fixture(9, 3);
        captured(&logs.stdout, b"one\ntwo\nthree\nfour\nfive\n", false);
        assert_eq!(logs.stdout.tail(3), "three\nfour\nfive");
        assert_eq!(logs.stdout.tail(0), "");
        assert_eq!(logs.stderr.tail(10), "");
    }

    #[test]
    fn backlog_and_subscription_join_a_partial_line_without_duplicates() {
        let (_dir, logs) = fixture(12, 4);
        captured(&logs.stdout, b"one\ntwo\npartial", false);
        let (backlog, mut live) = logs.stdout.subscribe(50);
        assert_eq!(backlog, ["one", "two"]);
        captured(&logs.stdout, b"-done\nnext\n", false);
        assert_eq!(live.try_recv().unwrap(), "partial-done");
        assert_eq!(live.try_recv().unwrap(), "next");
        assert!(live.try_recv().is_err());
        assert_eq!(logs.stdout.tail(2), "partial-done\nnext");
    }

    #[test]
    fn unfinished_lines_and_slow_subscribers_are_bounded() {
        let (_dir, logs) = fixture(13, 2);
        let (_, mut live) = logs.stdout.subscribe(0);
        captured(&logs.stdout, &vec![b'x'; MAX_LINE_BYTES * 3 + 7], false);
        assert_eq!(logs.stdout.state.lock().unwrap().pending.len(), 7);
        assert_eq!(live.try_recv().unwrap().len(), MAX_LINE_BYTES);
        retained(&logs.stdout);
        for _ in 0..LIVE_LINES + 5 {
            captured(&logs.stdout, b"line\n", false);
        }
        assert!(matches!(
            live.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
    }

    #[test]
    fn reset_removes_obsolete_segments_and_keeps_subscribers() {
        let (_dir, logs) = fixture(4, 5);
        captured(&logs.stdout, b"old\nold\nold\nold\nold\n", false);
        let (_, mut live) = logs.stdout.subscribe(0);
        let state = logs.stdout.state.lock().unwrap();
        let old = segment(&state.path, 4);
        assert!(old.exists());
        drop(state);
        logs.reset(LogRetentionConfig {
            max_file_size: 8,
            max_files: 1,
        });
        assert!(!old.exists());
        assert_eq!(logs.stdout.tail(10), "");
        captured(&logs.stdout, b"new\n", false);
        assert_eq!(live.try_recv().unwrap(), "new");
    }

    #[test]
    fn storage_failure_still_delivers_live_lines_and_final_fragments() {
        let (_dir, logs) = fixture(5, 2);
        let (_, mut live) = logs.stdout.subscribe(0);
        let path = logs.stdout.state.lock().unwrap().path.clone();
        std::fs::create_dir(segment(&path, 1)).unwrap(); // make rotation fail
        captured(&logs.stdout, b"first\nsecond\nlast", true);
        assert!(logs.stdout.state.lock().unwrap().failed);
        assert_eq!(live.try_recv().unwrap(), "first");
        assert_eq!(live.try_recv().unwrap(), "second");
        assert_eq!(live.try_recv().unwrap(), "last");
    }

    #[test]
    fn raw_output_preserves_binary_and_crlf() {
        let (_dir, logs) = fixture(4, 5);
        captured(&logs.stdout, b"\xff\r\none\r\n", true);
        assert_eq!(retained(&logs.stdout), b"\xff\r\none\r\n");
        assert_eq!(logs.stdout.tail(2), "\u{fffd}\none");
    }
}
