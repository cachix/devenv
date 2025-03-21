use futures::future::join_all;
use miette::Diagnostic;
use sqlx::SqlitePool;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{debug, trace};

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
}

type OnStderr = Box<dyn Fn(&InternalLog) + Send>;

pub struct CachedCommand<'a> {
    pool: &'a sqlx::SqlitePool,
    force_refresh: bool,
    extra_paths: Vec<PathBuf>,
    excluded_paths: Vec<PathBuf>,
    on_stderr: Option<OnStderr>,
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

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        let stdout_thread = std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut lines = stdout_reader.lines();
            while let Some(Ok(line)) = lines.next() {
                output.extend_from_slice(line.as_bytes());
                output.push(b'\n');
            }
            output
        });

        let on_stderr = self.on_stderr.take();

        let stderr_thread = std::thread::spawn(move || {
            let mut raw_lines: Vec<u8> = Vec::new();
            let mut ops = Vec::new();

            let mut lines = stderr_reader.lines();
            while let Some(Ok(line)) = lines.next() {
                if let Some(log) = InternalLog::parse(&line).and_then(Result::ok) {
                    if let Some(ref f) = &on_stderr {
                        f(&log);
                    }

                    if let Some(op) = extract_op_from_log_line(&log) {
                        ops.push(op);
                    }

                    // FIX: verbosity
                    if let Some(msg) = log
                        .filter_by_level(Verbosity::Info)
                        .and_then(InternalLog::get_msg)
                    {
                        raw_lines.extend_from_slice(msg.as_bytes());
                        raw_lines.push(b'\n');
                    }
                }
            }

            (ops, raw_lines)
        });

        let status = child.wait().map_err(CommandError::Io)?;
        let stdout = stdout_thread.join().unwrap();
        let (ops, stderr) = stderr_thread.join().unwrap();

        if !status.success() {
            return Ok(Output {
                status,
                stdout,
                stderr,
                ..Default::default()
            });
        }

        let mut env_inputs = Vec::new();
        let mut sources = Vec::new();

        for op in ops.into_iter() {
            match op {
                Op::CopiedSource { source, .. }
                | Op::EvaluatedFile { source }
                | Op::ReadFile { source }
                | Op::ReadDir { source }
                | Op::PathExists { source }
                | Op::TrackedPath { source }
                    if !self
                        .excluded_paths
                        .iter()
                        .any(|path| source.starts_with(path)) =>
                {
                    sources.push(source);
                }

                Op::GetEnv { name } => {
                    if let Ok(env_input) = EnvInputDesc::new(name) {
                        env_inputs.push(env_input);
                    }
                }

                _ => {}
            }
        }

        // Watch additional paths
        sources.extend_from_slice(&self.extra_paths);

        let now = SystemTime::now();
        let file_input_futures = sources
            .into_iter()
            .map(|source| {
                tokio::task::spawn_blocking(move || {
                    FileInputDesc::new(source, now).map_err(CommandError::Io)
                })
            })
            .collect::<Vec<_>>();

        let file_inputs = join_all(file_input_futures)
            .await
            .into_iter()
            .flatten()
            // TODO: add tracing here
            .filter_map(Result::ok)
            .collect::<Vec<_>>();

        let mut inputs = file_inputs
            .into_iter()
            .map(Input::File)
            .chain(env_inputs.into_iter().map(Input::Env))
            .collect::<Vec<_>>();

        inputs.sort();
        inputs.dedup();

        let input_hash = Input::compute_input_hash(&inputs);

        let _ = db::insert_command_with_inputs(
            self.pool,
            &raw_cmd,
            &cmd_hash,
            &input_hash,
            &stdout,
            &inputs,
        )
        .await
        .map_err(CommandError::Sqlx)?;

        Ok(Output {
            status,
            stdout,
            stderr,
            inputs,
            ..Default::default()
        })
    }
}

/// Check whether the command supports the flags required for caching.
pub fn supports_eval_caching(cmd: &Command) -> bool {
    cmd.get_program().to_string_lossy().ends_with("nix")
}

#[derive(Debug, Clone, Default)]
pub struct Output {
    /// The status code of the command.
    pub status: process::ExitStatus,
    /// The data that the process wrote to stdout.
    pub stdout: Vec<u8>,
    /// The data that the process wrote to stderr.
    pub stderr: Vec<u8>,
    /// A list of inputs that the command depends on and their hashes.
    pub inputs: Vec<Input>,
    /// Whether the output was returned from the cache or not.
    pub cache_hit: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Input {
    File(FileInputDesc),
    Env(EnvInputDesc),
}

impl Input {
    pub fn content_hash(&self) -> Option<&str> {
        match self {
            Self::File(desc) => desc.content_hash.as_deref(),
            Self::Env(desc) => desc.content_hash.as_deref(),
        }
    }

    pub fn compute_input_hash(inputs: &[Self]) -> String {
        inputs
            .iter()
            .filter_map(Input::content_hash)
            .collect::<String>()
    }

    pub fn partition_refs(inputs: &[Self]) -> (Vec<&FileInputDesc>, Vec<&EnvInputDesc>) {
        let mut file_inputs = Vec::new();
        let mut env_inputs = Vec::new();

        for input in inputs {
            match input {
                Self::File(desc) => file_inputs.push(desc),
                Self::Env(desc) => env_inputs.push(desc),
            }
        }

        (file_inputs, env_inputs)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInputDesc {
    pub path: PathBuf,
    pub is_directory: bool,
    pub content_hash: Option<String>,
    pub modified_at: SystemTime,
}

impl Ord for FileInputDesc {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.path.cmp(&other.path)
    }
}

impl PartialOrd for FileInputDesc {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl FileInputDesc {
    // A fallback system time is required for paths that don't exist.
    // This avoids duplicate entries for paths that don't exist and would only differ in terms of
    // the timestamp of when this function was called.
    pub fn new(path: PathBuf, fallback_system_time: SystemTime) -> Result<Self, io::Error> {
        let is_directory = path.is_dir();
        let content_hash = if is_directory {
            let paths = std::fs::read_dir(&path)?
                .filter_map(Result::ok)
                .map(|entry| entry.path().to_string_lossy().to_string())
                .collect::<String>();
            Some(hash::digest(&paths))
        } else {
            hash::compute_file_hash(&path).ok()
        };
        let modified_at = path
            .metadata()
            .and_then(|p| p.modified())
            .unwrap_or(fallback_system_time);
        Ok(Self {
            path,
            is_directory,
            content_hash,
            modified_at,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvInputDesc {
    pub name: String,
    pub content_hash: Option<String>,
}

impl Ord for EnvInputDesc {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for EnvInputDesc {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl EnvInputDesc {
    pub fn new(name: String) -> Result<Self, io::Error> {
        let value = std::env::var(&name).ok();
        let content_hash = value.map(hash::digest);
        Ok(Self { name, content_hash })
    }
}

impl From<db::FileInputRow> for Input {
    fn from(row: db::FileInputRow) -> Self {
        Self::File(row.into())
    }
}

impl From<db::EnvInputRow> for Input {
    fn from(row: db::EnvInputRow) -> Self {
        Self::Env(row.into())
    }
}

impl From<db::FileInputRow> for FileInputDesc {
    fn from(row: db::FileInputRow) -> Self {
        Self {
            path: row.path,
            is_directory: row.is_directory,
            content_hash: if row.content_hash.is_empty() {
                None
            } else {
                Some(row.content_hash)
            },
            modified_at: row.modified_at,
        }
    }
}

impl From<db::EnvInputRow> for EnvInputDesc {
    fn from(row: db::EnvInputRow) -> Self {
        Self {
            name: row.name,
            content_hash: if row.content_hash.is_empty() {
                None
            } else {
                Some(row.content_hash)
            },
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
        trace!(
            command_hash = cmd_hash,
            "Found cached command, checking input states"
        );
        let files = db::get_files_by_command_id(pool, cmd.id)
            .await
            .map_err(CommandError::Sqlx)?;

        let envs = db::get_envs_by_command_id(pool, cmd.id)
            .await
            .map_err(CommandError::Sqlx)?;

        let mut inputs = files
            .into_iter()
            .map(Input::from)
            .chain(envs.into_iter().map(Input::from))
            .collect::<Vec<_>>();

        inputs.sort();
        inputs.dedup();

        let mut should_refresh = false;

        let new_input_hash = Input::compute_input_hash(&inputs);

        // Hash of input hashes do not match
        if cmd.input_hash != new_input_hash {
            debug!(
                old_hash = cmd.input_hash,
                new_hash = new_input_hash,
                "Input hashes do not match, refreshing command",
            );
            should_refresh = true;
        }

        let inputs = Arc::new(inputs);

        if !should_refresh {
            let mut set = tokio::task::JoinSet::new();

            for (index, _) in inputs.iter().enumerate() {
                let inputs = Arc::clone(&inputs);
                set.spawn_blocking(move || match &inputs[index] {
                    Input::File(file) => {
                        let res = check_file_state(file);
                        (index, res)
                    }
                    Input::Env(env) => {
                        let res = check_env_state(env);
                        (index, res)
                    }
                });
            }

            while let Some(res) = set.join_next().await {
                if let Ok((index, Ok(file_state))) = res {
                    let input = &inputs[index];
                    match file_state {
                        FileState::MetadataModified { modified_at, .. } => {
                            if let Input::File(file) = &inputs[index] {
                                trace!(
                                    input = ?input,
                                    "File metadata has been modified, updating modified_at"
                                );
                                // TODO: batch with query builder?
                                db::update_file_modified_at(pool, &file.path, modified_at)
                                    .await
                                    .map_err(CommandError::Sqlx)?;
                            }
                        }
                        FileState::Modified { .. } => {
                            trace!(
                                input = ?input,
                                "Input has been modified, refreshing command"
                            );
                            should_refresh = true;
                        }
                        FileState::Removed { .. } => {
                            trace!(
                                input = ?input,
                                "Input has been removed, refreshing command"
                            );
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
            trace!("Command has not been modified, returning cached output");

            db::update_command_updated_at(pool, cmd.id)
                .await
                .map_err(CommandError::Sqlx)?;

            // No files have been modified, returning cached output
            Ok(Some(Output {
                status: process::ExitStatus::default(),
                stdout: cmd.output,
                stderr: Vec::new(),
                inputs: Arc::try_unwrap(inputs).unwrap_or_else(|arc| (*arc).clone()),
                cache_hit: true,
            }))
        }
    } else {
        trace!(command_hash = cmd_hash, "Command not found in cache");
        Ok(None)
    }
}

/// Convert a parse log line into into an `Op`.
/// Filters out paths that don't impact caching.
fn extract_op_from_log_line(log: &InternalLog) -> Option<Op> {
    match log {
        InternalLog::Msg { .. } => Op::from_internal_log(log).and_then(|op| match op {
            Op::EvaluatedFile { ref source }
            | Op::ReadFile { ref source }
            | Op::ReadDir { ref source }
            | Op::CopiedSource { ref source, .. }
            | Op::PathExists { ref source, .. }
            | Op::TrackedPath { ref source }
                if source.starts_with("/") && !source.starts_with("/nix/store") =>
            {
                Some(op)
            }
            Op::GetEnv { .. } => Some(op),
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
    Unchanged,
    /// The file's metadata, i.e. timestamp, has changed, but its content remains the same.
    MetadataModified { modified_at: SystemTime },
    /// The file's contents have been modified.
    Modified {
        new_hash: String,
        modified_at: SystemTime,
    },
    /// The file no longer exists in the file system.
    Removed,
}

fn check_file_state(file: &FileInputDesc) -> io::Result<FileState> {
    let metadata = match std::fs::metadata(&file.path) {
        Ok(metadata) => metadata,
        Err(_) => {
            if file.content_hash.is_some() {
                return Ok(FileState::Removed);
            } else {
                return Ok(FileState::Unchanged);
            }
        }
    };

    let modified_at = metadata.modified().and_then(truncate_to_seconds)?;
    if modified_at == file.modified_at {
        // File has not been modified
        return Ok(FileState::Unchanged);
    }

    // mtime has changed, check if content has changed
    let new_hash = if file.is_directory {
        if !metadata.is_dir() {
            return Ok(FileState::Removed);
        }

        let paths = std::fs::read_dir(&file.path)?
            .filter_map(Result::ok)
            .map(|entry| entry.path().to_string_lossy().to_string())
            .collect::<String>();
        hash::digest(&paths)
    } else {
        hash::compute_file_hash(&file.path)?
    };

    if Some(&new_hash) == file.content_hash.as_ref() {
        // File touched but hash unchanged
        Ok(FileState::MetadataModified { modified_at })
    } else {
        // Hash has changed, return new hash
        Ok(FileState::Modified {
            new_hash,
            modified_at,
        })
    }
}

fn check_env_state(env: &EnvInputDesc) -> io::Result<FileState> {
    let value = std::env::var(&env.name);

    if let Err(std::env::VarError::NotPresent) = value {
        if env.content_hash.is_none() {
            return Ok(FileState::Unchanged);
        } else {
            return Ok(FileState::Removed);
        }
    }

    let new_hash = hash::digest(value.unwrap_or("".into()));

    if Some(&new_hash) != env.content_hash.as_ref() {
        Ok(FileState::Modified {
            new_hash,
            modified_at: SystemTime::now(),
        })
    } else {
        Ok(FileState::Unchanged)
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

    fn create_file_row(dir: &TempDir, content: &[u8]) -> db::FileInputRow {
        let file_path = dir.path().join("test_file.txt");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(content).unwrap();

        let metadata = file_path.metadata().unwrap();
        let modified_at = metadata.modified().unwrap();
        let truncated_modified_at = truncate_to_seconds(modified_at).unwrap();
        let content_hash = hash::compute_file_hash(&file_path).unwrap();

        db::FileInputRow {
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
            check_file_state(&file_row.into()),
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
            check_file_state(&file_row.into()),
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
            check_file_state(&file_row.into()),
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
            check_file_state(&file_row.into()),
            Ok(FileState::Removed { .. })
        ));
    }
}
