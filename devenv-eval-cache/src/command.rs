use futures::future::join_all;
use miette::Diagnostic;
use sqlx::SqlitePool;
use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::fs;

use crate::{db, hash, internal_log::InternalLog, op::Op};

#[derive(Error, Diagnostic, Debug)]
pub enum CommandError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

pub struct CachedCommand<'a> {
    pool: &'a sqlx::SqlitePool,
    refresh: bool,
    extra_paths: Vec<PathBuf>,
    on_stderr: Option<Box<dyn Fn(&InternalLog) + Send>>,
}

impl<'a> CachedCommand<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self {
            pool,
            refresh: false,
            extra_paths: Vec::new(),
            on_stderr: None,
        }
    }

    /// Watch additional paths for changes.
    pub fn watch_path<P: AsRef<Path>>(&mut self, path: P) -> &mut Self {
        self.extra_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Force re-evaluation of the command.
    pub fn refresh(&mut self) -> &mut Self {
        self.refresh = true;
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
    pub async fn output(mut self, cmd: &'a mut Command) -> Result<process::Output, CommandError> {
        let raw_cmd = format!("{:?}", cmd);
        let cmd_hash = hash::digest(&raw_cmd);

        // Check whether the command has been previously run and the files it depends on have not been changed.
        if !self.refresh {
            if let Ok(Some(output)) = query_cached_output(self.pool, &cmd_hash).await {
                return Ok(process::Output {
                    status: process::ExitStatus::default(),
                    stdout: output.into_bytes(),
                    stderr: Vec::new(),
                });
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

            reader
                .lines()
                .map_while(Result::ok)
                .filter_map(|line| InternalLog::parse(line).and_then(|log| log.ok()))
                .inspect(|log| {
                    if let Some(ref f) = &on_stderr {
                        f(&log);
                    }
                })
                .filter_map(extract_op_from_log_line)
                .collect::<Vec<PathBuf>>()
        });

        let stdout_thread = tokio::spawn(async move {
            let mut output = Vec::new();
            stdout.read_to_end(&mut output).map(|_| output)
        });

        let status = child.wait().map_err(CommandError::Io)?;

        let output = stdout_thread.await.unwrap().map_err(CommandError::Io)?;
        let mut files = stderr_thread.await.unwrap();

        // Watch additional paths
        files.extend(self.extra_paths.iter().cloned());

        let path_hashes = join_all(files.into_iter().filter(|path| path.is_file()).map(|path| {
            tokio::spawn(async move {
                let hash = hash::compute_file_hash(&path)
                    .await
                    .map_err(CommandError::Io)?;
                let path = Cow::from(path);
                Ok((path, hash))
            })
        }))
        .await
        .into_iter()
        .flatten()
        .collect::<Result<Vec<(Cow<'_, Path>, String)>, CommandError>>()?;

        let _ =
            db::insert_command_with_files(self.pool, &raw_cmd, &cmd_hash, &output, &path_hashes)
                .await
                .map_err(CommandError::Sqlx)?;

        Ok(process::Output {
            status,
            stdout: output,
            stderr: Vec::new(),
        })
    }
}

/// Represents the state of a file in the cache system.
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
    /// The file's content has been modified.
    Modified {
        path: PathBuf,
        new_hash: String,
        modified_at: SystemTime,
    },
    /// The file no longer exists in the file system.
    Removed { path: PathBuf },
}

/// Try to fetch the cached output for a hashed command.
///
/// Returns the cached output if the command has been cached and none of the file dependencies have
/// been updated.
async fn query_cached_output(
    pool: &SqlitePool,
    cmd_hash: &str,
) -> Result<Option<String>, CommandError> {
    let cached_cmd = db::get_command_by_hash(pool, cmd_hash)
        .await
        .map_err(CommandError::Sqlx)?;

    if let Some(db::CommandRow { id, output, .. }) = cached_cmd {
        let files = db::get_files_by_command_id(pool, id)
            .await
            .map_err(CommandError::Sqlx)?;

        let file_checks = files
            .into_iter()
            .map(|file| tokio::spawn(check_file_state(file)));

        let updated_files = join_all(file_checks)
            .await
            .into_iter()
            .filter_map(|result| result.ok().and_then(|r| r.ok()))
            .filter(|state| {
                matches!(
                    state,
                    FileState::Modified { .. } | FileState::Removed { .. }
                )
            })
            .collect::<Vec<_>>();

        if updated_files.is_empty() {
            // No files have been modified, returning cached output
            Ok(Some(output))
        } else {
            // Command not cached
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

fn extract_op_from_log_line(log: InternalLog) -> Option<PathBuf> {
    match log {
        InternalLog::Msg { .. } => Op::from_internal_log(&log).and_then(|op| match op {
            Op::EvaluatedFile { source }
            | Op::ReadFile { source }
            | Op::CopiedSource { source, .. }
            | Op::TrackedPath { source }
                if source.starts_with("/") && !source.starts_with("/nix/store") =>
            {
                Some(source)
            }
            _ => None,
        }),
        _ => None,
    }
}

fn system_time_to_unix_timestamp(time: SystemTime) -> std::io::Result<u64> {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

async fn check_file_state(file: db::FilePathRow) -> std::io::Result<FileState> {
    let metadata = match fs::metadata(&file.path).await {
        Ok(metadata) => metadata,
        Err(_) => return Ok(FileState::Removed { path: file.path }),
    };

    let metadata_modified = metadata.modified()?;
    let file_modified = system_time_to_unix_timestamp(metadata_modified)?;
    let updated_at = system_time_to_unix_timestamp(file.updated_at)?;

    if file_modified <= updated_at {
        // File has not been modified
        return Ok(FileState::Unchanged { path: file.path });
    }

    // File has been modified, recompute the hash
    let new_hash = hash::compute_file_hash(&file.path).await?;

    if new_hash != file.content_hash {
        // Hash has changed, return updated information
        Ok(FileState::Modified {
            path: file.path,
            new_hash,
            modified_at: metadata_modified,
        })
    } else {
        // File modified but hash unchanged
        Ok(FileState::MetadataModified {
            path: file.path,
            modified_at: metadata_modified,
        })
    }
}

/// Normalize a path by stripping leading slashes.
fn normalize_path(path: PathBuf) -> PathBuf {
    path.strip_prefix("/").unwrap_or(&path).to_path_buf()
}
