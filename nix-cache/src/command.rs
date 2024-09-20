use futures::future::join_all;
use miette::Diagnostic;
use sqlx::SqlitePool;
use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::UNIX_EPOCH;
use thiserror::Error;
use tokio::fs;

use crate::{
    db, hash,
    nix_internal_log::{parse_log_line, NixInternalLog},
    op::Op,
};

#[derive(Error, Diagnostic, Debug)]
pub enum CommandError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Clone, Debug, Default)]
pub struct CommandOptions {
    pub force: bool,
}

pub struct CachedCommand<'a> {
    pool: &'a sqlx::SqlitePool,
    options: CommandOptions,
    pub inner: Command,
}

impl<'a> CachedCommand<'a> {
    /// Run a (Nix) command with caching enabled.
    ///
    /// If the command has been run before and the files it depends on have not been modified,
    /// the cached output will be returned.
    ///
    /// Pass `force = true` to force re-evaluation of the command.
    pub fn new(pool: &'a SqlitePool, mut cmd: Command, options: CommandOptions) -> Self {
        cmd.arg("-vv")
            .arg("--log-format")
            .arg("internal-json")
            .arg("--json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        Self {
            pool,
            options,
            inner: cmd,
        }
    }

    pub async fn run(&mut self) -> Result<process::Output, CommandError> {
        let raw_cmd = format!("{:?}", self.inner);
        let cmd_hash = hash::digest(&raw_cmd);

        if !self.options.force {
            if let Ok(Some(output)) = query_cached_output(self.pool, &cmd_hash).await {
                return Ok(process::Output {
                    status: process::ExitStatus::default(),
                    stdout: output.into_bytes(),
                    stderr: Vec::new(),
                });
            }
        }

        let mut child = self.inner.spawn().map_err(CommandError::Io)?;

        let mut stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stderr_thread = tokio::spawn(async move {
            let reader = BufReader::new(stderr);

            reader
                .lines()
                .map_while(Result::ok)
                .filter_map(extract_op_from_log_line)
                .collect::<Vec<PathBuf>>()
        });

        let stdout_thread = tokio::spawn(async move {
            let mut output = Vec::new();
            let res = stdout.read_to_end(&mut output);
            res.map(|_| output)
        });

        let status = child.wait().map_err(CommandError::Io)?;

        let output = stdout_thread.await.unwrap().map_err(CommandError::Io)?;
        let files = stderr_thread.await.unwrap();

        println!("files: {:?}", files);

        let path_hashes = join_all(files.into_iter().map(|path| {
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

        let (id, _) =
            db::insert_command_with_files(self.pool, &raw_cmd, &cmd_hash, &output, &path_hashes)
                .await
                .map_err(CommandError::Sqlx)?;

        println!(
            r#"
        id: {id}
        command: {raw_cmd}
        hash: {cmd_hash}
        files: {path_hashes:?}
        "#
        );

        Ok(process::Output {
            status,
            stdout: output,
            stderr: Vec::new(),
        })
    }
}

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

        let file_checks = files.into_iter().map(
            |db::FileRow {
                 path,
                 content_hash,
                 updated_at,
             }| {
                tokio::spawn(async move {
                    let metadata = match fs::metadata(&path).await {
                        Ok(metadata) => metadata,
                        Err(_) => return None,
                    };

                    let file_modified = metadata.modified().ok()?;
                    let file_modified = file_modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
                    let updated_at = updated_at.duration_since(UNIX_EPOCH).ok()?.as_secs();

                    if file_modified <= updated_at {
                        return None; // File has not been modified
                    }

                    // File has been modified, recompute hash
                    let new_hash = match hash::compute_file_hash(&path).await {
                        Ok(hash) => hash,
                        Err(_) => return None, // Unable to compute hash
                    };

                    if new_hash != content_hash {
                        // Hash has changed, return updated information
                        Some((path, new_hash, file_modified))
                    } else {
                        println!("File modified but hash unchanged: {:?}", path);
                        None // File modified but hash unchanged
                    }
                })
            },
        );

        let updated_files = join_all(file_checks)
            .await
            .into_iter()
            .filter_map(|result| result.ok().flatten())
            .collect::<Vec<_>>();

        println!("updated_files: {:?}", updated_files);

        if updated_files.is_empty() {
            println!("No files have been modified, returning cached output");
            Ok(Some(output))
        } else {
            println!("Command not cached, inserting new command and files");
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

fn extract_op_from_log_line(line: String) -> Option<PathBuf> {
    parse_log_line(line)
        .and_then(|log| log.ok())
        .and_then(|log| match log {
            NixInternalLog::Msg { .. } => Op::from_internal_log(&log),
            _ => None,
        })
        .and_then(|op| {
            println!("{:?}", op);

            match op {
                Op::EvaluatedFile { source }
                | Op::ReadFile { source }
                | Op::CopiedSource { source, .. }
                | Op::TrackedPath { source }
                    if source.starts_with("/") =>
                {
                    Some(normalize_path(source))
                }
                _ => None,
            }
        })
}

/// Normalize a path by stripping leading slashes.
fn normalize_path(path: PathBuf) -> PathBuf {
    path.strip_prefix("/").unwrap_or(&path).to_path_buf()
}
