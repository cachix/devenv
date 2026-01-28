//! Evaluation operation types and parsing.
//!
//! This module provides `EvalOp` for tracking filesystem/environment operations
//! during Nix evaluation, along with parsing logic to extract them from Nix logs.
//!
//! Note: `EvalOp` is duplicated in `devenv_activity::EvalOp`.

use crate::internal_log::InternalLog;
use regex::Regex;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

/// A filesystem or environment operation observed during Nix evaluation.
///
/// These operations are used for cache invalidation and dependency tracking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalOp {
    /// Copied a file to the Nix store.
    CopiedSource { source: PathBuf, target: PathBuf },
    /// Evaluated a Nix file.
    EvaluatedFile { source: PathBuf },
    /// Read a file's contents with `builtins.readFile`.
    ReadFile { source: PathBuf },
    /// List a directory's contents with `builtins.readDir`.
    ReadDir { source: PathBuf },
    /// Read an environment variable with `builtins.getEnv`.
    GetEnv { name: String },
    /// Check that a file exists with `builtins.pathExists`.
    PathExists { source: PathBuf },
    /// Used a tracked devenv string path.
    TrackedPath { source: PathBuf },
}

/// Convert to the activity event type for serialization.
impl From<EvalOp> for devenv_activity::EvalOp {
    fn from(op: EvalOp) -> Self {
        match op {
            EvalOp::CopiedSource { source, target } => {
                devenv_activity::EvalOp::CopiedSource { source, target }
            }
            EvalOp::EvaluatedFile { source } => devenv_activity::EvalOp::EvaluatedFile { source },
            EvalOp::ReadFile { source } => devenv_activity::EvalOp::ReadFile { source },
            EvalOp::ReadDir { source } => devenv_activity::EvalOp::ReadDir { source },
            EvalOp::GetEnv { name } => devenv_activity::EvalOp::GetEnv { name },
            EvalOp::PathExists { source } => devenv_activity::EvalOp::PathExists { source },
            EvalOp::TrackedPath { source } => devenv_activity::EvalOp::TrackedPath { source },
        }
    }
}

// Regex patterns for parsing operations from log messages
static EVALUATED_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^evaluating file '(?P<source>.*)'( \\(cached\\))?$").expect("invalid regex")
});
static COPIED_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^copied source '(?P<source>.*)' -> '(?P<target>.*)'$").expect("invalid regex")
});
static READ_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^devenv readFile: '(?P<source>.*)'$").expect("invalid regex"));
static READ_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^devenv readDir: '(?P<source>.*)'$").expect("invalid regex"));
static GET_ENV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^devenv getEnv: '(?P<name>.*)'$").expect("invalid regex"));
static PATH_EXISTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^devenv pathExists: '(?P<source>.*)'$").expect("invalid regex"));
static TRACKED_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^trace: devenv path: '(?P<source>.*)'$").expect("invalid regex"));

impl EvalOp {
    /// Extract an `EvalOp` from an `InternalLog`.
    ///
    /// This parses Nix log messages to detect file/env operations that occurred
    /// during evaluation. These operations are used for cache invalidation.
    pub fn from_internal_log(log: &InternalLog) -> Option<Self> {
        match log {
            InternalLog::Msg { msg, .. } => {
                if let Some(matches) = COPIED_SOURCE.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    let target = PathBuf::from(&matches["target"]);
                    Some(EvalOp::CopiedSource { source, target })
                } else if let Some(matches) = EVALUATED_FILE.captures(msg) {
                    let mut source = PathBuf::from(&matches["source"]);
                    // If the evaluated file is a directory, we assume that the file is `default.nix`.
                    if source.is_dir() {
                        source.push("default.nix");
                    }
                    Some(EvalOp::EvaluatedFile { source })
                } else if let Some(matches) = READ_FILE.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(EvalOp::ReadFile { source })
                } else if let Some(matches) = READ_DIR.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(EvalOp::ReadDir { source })
                } else if let Some(matches) = GET_ENV.captures(msg) {
                    let name = matches["name"].to_string();
                    Some(EvalOp::GetEnv { name })
                } else if let Some(matches) = PATH_EXISTS.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(EvalOp::PathExists { source })
                } else if let Some(matches) = TRACKED_PATH.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(EvalOp::TrackedPath { source })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Observer trait for receiving evaluation operations.
///
/// Implementations of this trait can be registered with `NixLogBridge`
/// to receive notifications about file/env operations during evaluation.
///
/// This trait uses `Arc<Self>` pattern to support shared ownership,
/// which is necessary because observers may be stored and invoked from
/// multiple contexts (e.g., across thread boundaries in FFI callbacks).
pub trait OpObserver: Send + Sync + 'static {
    /// Called when an operation is observed during evaluation.
    ///
    /// Implementations should be efficient as this is called synchronously
    /// from the log processing path.
    fn on_op(&self, op: EvalOp);

    /// Check if the observer is still active and accepting operations.
    ///
    /// Returns `false` to indicate the observer should be removed or skipped.
    fn is_active(&self) -> bool;
}

/// Wrapper to allow `Arc<dyn OpObserver>` to implement `OpObserver`
impl OpObserver for Arc<dyn OpObserver> {
    fn on_op(&self, op: EvalOp) {
        (**self).on_op(op);
    }

    fn is_active(&self) -> bool {
        (**self).is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_log::Verbosity;

    fn create_log(msg: &str) -> InternalLog {
        InternalLog::Msg {
            msg: msg.to_string(),
            raw_msg: None,
            level: Verbosity::Warn,
        }
    }

    #[test]
    fn test_copied_source() {
        let log = create_log("copied source '/path/to/source' -> '/path/to/target'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::CopiedSource {
                source: PathBuf::from("/path/to/source"),
                target: PathBuf::from("/path/to/target"),
            })
        );
    }

    #[test]
    fn test_evaluated_file() {
        let log = create_log("evaluating file '/path/to/file'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::EvaluatedFile {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_evaluated_file_cached() {
        let log = create_log("evaluating file '/path/to/file' (cached)");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::EvaluatedFile {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_read_file() {
        let log = create_log("devenv readFile: '/path/to/file'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::ReadFile {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_read_dir() {
        let log = create_log("devenv readDir: '/path/to/dir'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::ReadDir {
                source: PathBuf::from("/path/to/dir"),
            })
        );
    }

    #[test]
    fn test_get_env() {
        let log = create_log("devenv getEnv: 'SOME_ENV'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::GetEnv {
                name: "SOME_ENV".to_string(),
            })
        );
    }

    #[test]
    fn test_path_exists() {
        let log = create_log("devenv pathExists: '/path/to/file'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::PathExists {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_tracked_path() {
        let log = create_log("trace: devenv path: '/path/to/file'");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(
            op,
            Some(EvalOp::TrackedPath {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_unmatched_log() {
        let log = create_log("some unrelated message");
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(op, None);
    }

    #[test]
    fn test_non_msg_log() {
        let log = InternalLog::Stop { id: 1 };
        let op = EvalOp::from_internal_log(&log);
        assert_eq!(op, None);
    }
}
