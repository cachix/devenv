use futures::future::join_all;
use miette::Diagnostic;
use sqlx::SqlitePool;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::{
    db, hash,
    internal_log::{InternalLog, Verbosity},
    op::Op,
};

#[derive(Error, Diagnostic, Debug)]
pub enum CommandError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("Nix command failed: {0}")]
    NonZeroExitStatus(process::ExitStatus),
}

pub struct CachedCommand<'a> {
    pool: &'a sqlx::SqlitePool,
    force_refresh: bool,
    extra_paths: Vec<PathBuf>,
    excluded_paths: Vec<PathBuf>,
    on_stderr: Option<Box<dyn Fn(&InternalLog) + Send>>,
}

impl<'a> CachedCommand<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self {
            pool,
            force_refresh: false,
            extra_paths: Vec::new(),
            excluded_paths: Vec::new(),
            on_stderr: None,
        }
    }

    /// Watch additional paths for changes.
    pub fn watch_path<P: AsRef<Path>>(&mut self, path: P) -> &mut Self {
        self.extra_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Remove a path from being watched for changes.
    pub fn unwatch_path<P: AsRef<Path>>(&mut self, path: P) -> &mut Self {
        self.excluded_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Force re-evaluation of the command.
    pub fn force_refresh(&mut self) -> &mut Self {
        self.force_refresh = true;
        self
    }

    pub fn on_stderr<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&InternalLog) + Send + 'static,
    {
        self.on_stderr = Some(Box::new(f));
        self
    }

    /// Run a (Nix) command with caching enabled.
    ///
    /// If the command has been run before and the files it depends on have not been modified,
    /// the cached output will be returned.
    pub async fn output(mut self, cmd: &'a mut Command) -> Result<Output, CommandError> {
        let raw_cmd = format!("{:?}", cmd);
        let cmd_hash = hash::digest(&raw_cmd);

        // Check whether the command has been previously run and the files it depends on have not been changed.
        if !self.force_refresh {
            if let Ok(Some(output)) = query_cached_output(self.pool, &cmd_hash).await {
                return Ok(output);
            }
        }

        cmd.arg("-vv")
            .arg("--log-format")
            .arg("internal-json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(CommandError::Io)?;

        let mut stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let on_stderr = self.on_stderr.take();

        let stderr_thread = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut raw_lines: Vec<u8> = Vec::new();
            let mut ops = Vec::new();

            let mut lines = reader.lines();
            while let Some(Ok(line)) = lines.next() {
                if let Some(log) = InternalLog::parse(&line).and_then(Result::ok) {
                    if let Some(ref f) = &on_stderr {
                        f(&log);
                    }

                    // FIX: verbosity
                    if let Some(msg) = log.get_log_msg_by_level(Verbosity::Info) {
                        raw_lines.extend_from_slice(msg.as_bytes());
                    }

                    if let Some(op) = extract_op_from_log_line(log) {
                        ops.push(op);
                    }
                }
            }

            (ops, raw_lines)
        });

        let stdout_thread = tokio::spawn(async move {
            let mut output = Vec::new();
            stdout.read_to_end(&mut output).map(|_| output)
        });

        let status = child.wait().map_err(CommandError::Io)?;

        if !status.success() {
            return Err(CommandError::NonZeroExitStatus(status));
        }

        let stdout = stdout_thread.await.unwrap().map_err(CommandError::Io)?;
        let (mut ops, stderr) = stderr_thread.await.unwrap();

        // Remove excluded paths if any are a parent directory
        ops.retain_mut(|op| {
            !self
                .excluded_paths
                .iter()
                .any(|path| op.source().starts_with(path))
        });

        // Convert Ops to FilePaths
        let mut file_path_futures = ops
            .into_iter()
            .map(|op| {
                tokio::task::spawn_blocking(move || {
                    FilePath::new(op.source().to_path_buf()).map_err(CommandError::Io)
                })
            })
            .collect::<Vec<_>>();

        // Watch additional paths
        file_path_futures.extend(self.extra_paths.into_iter().map(|path| {
            tokio::task::spawn_blocking(move || FilePath::new(path).map_err(CommandError::Io))
        }));

        let mut file_paths = join_all(file_path_futures)
            .await
            .into_iter()
            .flatten()
            // TODO: add tracing here
            .filter_map(Result::ok)
            .collect::<Vec<_>>();

        file_paths.sort_by(|a, b| a.path.cmp(&b.path));
        file_paths.dedup();

        let input_hash = hash::digest(
            &file_paths
                .iter()
                .map(|p| p.content_hash.clone())
                .collect::<String>(),
        );

        let _ = db::insert_command_with_files(
            self.pool,
            &raw_cmd,
            &cmd_hash,
            &input_hash,
            &stdout,
            &file_paths,
        )
        .await
        .map_err(CommandError::Sqlx)?;

        Ok(Output {
            status,
            stdout,
            stderr,
            paths: file_paths,
        })
    }
}

/// Check whether the command supports the flags required for caching.
pub fn supports_eval_caching(cmd: &Command) -> bool {
    cmd.get_program().to_string_lossy().ends_with("nix")
}

#[derive(Debug)]
pub struct Output {
    /// The status code of the command.
    pub status: process::ExitStatus,
    /// The data that the process wrote to stdout.
    pub stdout: Vec<u8>,
    /// The data that the process wrote to stderr.
    pub stderr: Vec<u8>,
    /// A list of paths that the command depends on and their hashes.
    pub paths: Vec<FilePath>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilePath {
    pub path: PathBuf,
    pub is_directory: bool,
    pub content_hash: String,
    pub modified_at: SystemTime,
}

impl FilePath {
    pub fn new(path: PathBuf) -> Result<Self, io::Error> {
        let is_directory = path.is_dir();
        let content_hash = if is_directory {
            let paths = std::fs::read_dir(&path)?
                .filter_map(Result::ok)
                .map(|entry| entry.path().to_string_lossy().to_string())
                .collect::<String>();
            hash::digest(&paths)
        } else {
            hash::compute_file_hash(&path)?
        };
        let modified_at = path.metadata()?.modified()?;
        Ok(Self {
            path,
            is_directory,
            content_hash,
            modified_at,
        })
    }
}

impl From<db::FilePathRow> for FilePath {
    fn from(row: db::FilePathRow) -> Self {
        Self {
            path: row.path,
            is_directory: row.is_directory,
            content_hash: row.content_hash,
            modified_at: row.modified_at,
        }
    }
}

/// Try to fetch the cached output for a hashed command.
///
/// Returns the cached output if the command has been cached and none of the file dependencies have
/// been updated.
async fn query_cached_output(
    pool: &SqlitePool,
    cmd_hash: &str,
) -> Result<Option<Output>, CommandError> {
    let cached_cmd = db::get_command_by_hash(pool, cmd_hash)
        .await
        .map_err(CommandError::Sqlx)?;

    if let Some(cmd) = cached_cmd {
        let mut files = db::get_files_by_command_id(pool, cmd.id)
            .await
            .map_err(CommandError::Sqlx)?;

        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.dedup();

        let mut should_refresh = false;

        let file_input_hash = hash::digest(
            &files
                .iter()
                .map(|f| f.content_hash.clone())
                .collect::<String>(),
        );

        // Hash of input hashes do not match
        if cmd.input_hash != file_input_hash {
            should_refresh = true;
        }

        if !should_refresh {
            let mut set = tokio::task::JoinSet::new();

            for file in &files {
                let file = file.clone();
                set.spawn_blocking(|| check_file_state(file));
            }

            while let Some(res) = set.join_next().await {
                if let Ok(Ok(file_state)) = res {
                    match file_state {
                        FileState::MetadataModified {
                            modified_at, path, ..
                        } => {
                            // TODO: batch with query builder?
                            db::update_file_modified_at(pool, path, modified_at)
                                .await
                                .map_err(CommandError::Sqlx)?;
                        }
                        FileState::Modified { .. } => {
                            should_refresh = true;
                        }
                        FileState::Removed { .. } => {
                            should_refresh = true;
                        }
                        _ => (),
                    }
                }
            }
        };

        if should_refresh {
            Ok(None)
        } else {
            db::update_command_updated_at(pool, cmd.id)
                .await
                .map_err(CommandError::Sqlx)?;

            // No files have been modified, returning cached output
            Ok(Some(Output {
                status: process::ExitStatus::default(),
                stdout: cmd.output,
                stderr: Vec::new(),
                paths: files.into_iter().map(FilePath::from).collect(),
            }))
        }
    } else {
        Ok(None)
    }
}

/// Convert a parse log line into into an `Op`.
/// Filters out paths that don't impact caching.
fn extract_op_from_log_line(log: InternalLog) -> Option<Op> {
    match log {
        InternalLog::Msg { .. } => Op::from_internal_log(&log).and_then(|op| match op {
            Op::EvaluatedFile { ref source }
            | Op::ReadFile { ref source }
            | Op::ReadDir { ref source }
            | Op::CopiedSource { ref source, .. }
            | Op::TrackedPath { ref source }
                if source.starts_with("/") && !source.starts_with("/nix/store") =>
            {
                Some(op)
            }
            _ => None,
        }),
        _ => None,
    }
}

/// Represents the various states of "modified" that we care about.
#[derive(Debug)]
#[allow(dead_code)]
enum FileState {
    /// The file has not been modified since it was last cached.
    Unchanged { path: PathBuf },
    /// The file's metadata, i.e. timestamp, has changed, but its content remains the same.
    MetadataModified {
        path: PathBuf,
        modified_at: SystemTime,
    },
    /// The file's contents have been modified.
    Modified {
        path: PathBuf,
        new_hash: String,
        modified_at: SystemTime,
    },
    /// The file no longer exists in the file system.
    Removed { path: PathBuf },
}

fn check_file_state(file: db::FilePathRow) -> io::Result<FileState> {
    let metadata = match std::fs::metadata(&file.path) {
        Ok(metadata) => metadata,
        // Fix
        Err(_) => return Ok(FileState::Removed { path: file.path }),
    };

    let modified_at = metadata.modified().and_then(truncate_to_seconds)?;
    if modified_at == file.modified_at {
        // File has not been modified
        return Ok(FileState::Unchanged { path: file.path });
    }

    // mtime has changed, check if content has changed
    let new_hash = if file.is_directory {
        if !metadata.is_dir() {
            return Ok(FileState::Removed { path: file.path });
        }

        let paths = std::fs::read_dir(&file.path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path().to_string_lossy().to_string())
            .collect::<String>();
        hash::digest(&paths)
    } else {
        hash::compute_file_hash(&file.path)?
    };

    if new_hash == file.content_hash {
        // File touched but hash unchanged
        Ok(FileState::MetadataModified {
            path: file.path,
            modified_at,
        })
    } else {
        // Hash has changed, return new hash
        Ok(FileState::Modified {
            path: file.path,
            new_hash,
            modified_at,
        })
    }
}

fn truncate_to_seconds(time: SystemTime) -> io::Result<SystemTime> {
    let duration_since_epoch = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "SystemTime before UNIX EPOCH"))?;

    let seconds = duration_since_epoch.as_secs();
    Ok(UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempdir::TempDir;

    fn create_file_row(dir: &TempDir, content: &[u8]) -> db::FilePathRow {
        let file_path = dir.path().join("test_file.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(content).unwrap();

        let metadata = file_path.metadata().unwrap();
        let modified_at = metadata.modified().unwrap();
        let truncated_modified_at = truncate_to_seconds(modified_at).unwrap();
        let content_hash = hash::compute_file_hash(&file_path).unwrap();

        db::FilePathRow {
            path: file_path,
            is_directory: false,
            content_hash,
            modified_at: truncated_modified_at,
            updated_at: truncated_modified_at,
        }
    }

    #[test]
    fn test_unchanged_file() {
        let temp_dir = TempDir::new("test_unchanged_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        assert!(matches!(
            check_file_state(file_row),
            Ok(FileState::Unchanged { .. })
        ));
    }

    #[test]
    fn test_metadata_modified_file() {
        let temp_dir = TempDir::new("test_metadata_modified_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        // Sleep to ensure the new modification time is different
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Update the file's timestamp
        let new_time = SystemTime::now();
        let file = File::open(&file_row.path).unwrap();
        file.set_modified(new_time).unwrap();
        drop(file);

        assert!(matches!(
            check_file_state(file_row),
            Ok(FileState::MetadataModified { .. })
        ));
    }

    #[test]
    fn test_content_modified_file() {
        let temp_dir = TempDir::new("test_content_modified_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        std::thread::sleep(std::time::Duration::from_secs(1));

        // Modify the file contents
        let mut file = File::create(&file_row.path).unwrap();
        file.write_all(b"Modified content").unwrap();

        assert!(matches!(
            check_file_state(file_row),
            Ok(FileState::Modified { .. })
        ));
    }

    #[test]
    fn test_removed_file() {
        let temp_dir = TempDir::new("test_removed_file").unwrap();
        let file_row = create_file_row(&temp_dir, b"Hello, World!");

        // Remove the file
        std::fs::remove_file(&file_row.path).unwrap();

        assert!(matches!(
            check_file_state(file_row),
            Ok(FileState::Removed { .. })
        ));
    }
}
