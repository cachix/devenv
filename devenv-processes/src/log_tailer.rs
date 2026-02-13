use std::path::PathBuf;
use std::time::Duration;

use devenv_activity::Activity;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use tokio::task::JoinHandle;
use tracing::debug;

/// Spawn a task that tails a log file and emits lines to an activity.
pub fn spawn_file_tailer(path: PathBuf, activity: Activity, is_stderr: bool) -> JoinHandle<()> {
    tokio::spawn(async move {
        // File is already created/truncated by start_command before job starts
        let file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) => {
                debug!("Failed to open log file {}: {}", path.display(), e);
                return;
            }
        };

        let mut position: u64 = 0;
        let mut reader = BufReader::new(file).lines();

        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    position += line.len() as u64 + 1;
                    if is_stderr {
                        activity.error(&line);
                    } else {
                        activity.log(&line);
                    }
                }
                Ok(None) => {
                    // EOF reached, wait a bit and try again (tail -f behavior)
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    let new_file = match tokio::fs::File::open(&path).await {
                        Ok(f) => f,
                        Err(_) => break,
                    };

                    // Reset position if file was truncated (e.g., during restart)
                    let metadata = match new_file.metadata().await {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    if metadata.len() < position {
                        position = 0;
                    }

                    let mut new_file = new_file;
                    if let Err(e) = new_file.seek(std::io::SeekFrom::Start(position)).await {
                        debug!("Failed to seek in log file {}: {}", path.display(), e);
                        break;
                    }

                    reader = BufReader::new(new_file).lines();
                }
                Err(e) => {
                    debug!("Error reading log file {}: {}", path.display(), e);
                    break;
                }
            }
        }
    })
}
