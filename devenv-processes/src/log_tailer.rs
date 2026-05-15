use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

use devenv_activity::ActivityRef;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::task::JoinHandle;
use tracing::debug;

/// Spawn a task that tails a log file and emits lines to an activity.
pub fn spawn_file_tailer(path: PathBuf, activity: ActivityRef, is_stderr: bool) -> JoinHandle<()> {
    tokio::spawn(async move {
        // File is already created/truncated by start_command before job starts
        let file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) => {
                debug!("Failed to open log file {}: {}", path.display(), e);
                return;
            }
        };

        let mut ino = file.metadata().await.map(|m| m.ino()).unwrap_or(0);
        let mut reader = BufReader::new(file);

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    // EOF — check for truncation or replacement before sleeping
                    let position = reader.stream_position().await.unwrap_or(0);

                    tokio::time::sleep(Duration::from_millis(100)).await;

                    let meta = match tokio::fs::metadata(&path).await {
                        Ok(m) => m,
                        Err(_) => break,
                    };

                    if meta.ino() != ino {
                        // File was replaced (e.g., process restart created a new file).
                        // Re-open from the beginning.
                        let file = match tokio::fs::File::open(&path).await {
                            Ok(f) => f,
                            Err(_) => break,
                        };
                        ino = meta.ino();
                        reader = BufReader::new(file);
                    } else if meta.len() < position {
                        // Same file but truncated — seek back to the start.
                        if reader.seek(std::io::SeekFrom::Start(0)).await.is_err() {
                            break;
                        }
                    }
                    // Otherwise the file just hasn't grown yet; loop will re-read.
                }
                Ok(_) => {
                    // Drop CR/LF. Under a pty the OS translates "\n" writes to
                    // "\r\n", so log files commonly have CRLF line endings; any
                    // bare "\r" surviving into the TUI moves the terminal cursor
                    // back to column 0 mid-row and erases rendered content.
                    // Progress-style writers also emit mid-line "\r" between
                    // updates, which get bundled into one read_line record.
                    line.retain(|c| c != '\r' && c != '\n');
                    if is_stderr {
                        activity.error(&line);
                    } else {
                        activity.log(&line);
                    }
                }
                Err(e) => {
                    debug!("Error reading log file {}: {}", path.display(), e);
                    break;
                }
            }
        }
    })
}
