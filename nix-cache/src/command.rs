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
    nix_internal_log::{NixInternalLog, NixVerbosity},
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
    extra_paths: Vec<PathBuf>,
}

impl<'a> CachedCommand<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self {
            pool,
            options: CommandOptions::default(),
            extra_paths: Vec::new(),
        }
    }

    /// Watch additional paths for changes.
    pub fn watch_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.extra_paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Force re-evaluation of the command.
    pub fn force(mut self, force: bool) -> Self {
        self.options.force = force;
        self
    }

    /// Run a (Nix) command with caching enabled.
    ///
    /// If the command has been run before and the files it depends on have not been modified,
    /// the cached output will be returned.
    pub async fn run(&mut self, mut cmd: &'a mut Command) -> Result<process::Output, CommandError> {
        let raw_cmd = format!("{:?}", cmd);
        let cmd_hash = hash::digest(&raw_cmd);

        // Check whether the command has been previously run and the files it depends on have not been changed.
        if !self.options.force {
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
            .arg("--json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(CommandError::Io)?;

        let mut stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stderr_thread = tokio::spawn(async move {
            let reader = BufReader::new(stderr);

            reader
                .lines()
                .map_while(Result::ok)
                .filter_map(|line| NixInternalLog::parse(line).and_then(|log| log.ok()))
                .inspect(|log| {
                    if let NixInternalLog::Msg { msg, level, .. } = log {
                        if *level <= NixVerbosity::Info {
                            eprintln!("{msg}");
                        }
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

        eprintln!("files: {:?}", files);

        // Watch additional paths
        files.extend(self.extra_paths.iter().cloned());

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

        eprintln!(
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
                        eprintln!("File modified but hash unchanged: {:?}", path);
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

        eprintln!("updated_files: {:?}", updated_files);

        if updated_files.is_empty() {
            eprintln!("No files have been modified, returning cached output");
            Ok(Some(output))
        } else {
            eprintln!("Command not cached, inserting new command and files");
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

fn extract_op_from_log_line(log: NixInternalLog) -> Option<PathBuf> {
    match log {
        NixInternalLog::Msg { .. } => Op::from_internal_log(&log).and_then(|op| match op {
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

/// Normalize a path by stripping leading slashes.
fn normalize_path(path: PathBuf) -> PathBuf {
    path.strip_prefix("/").unwrap_or(&path).to_path_buf()
}
