//! Evaluation operation types and structured Nix effect parsing.
//!
//! Nix emits evaluation dependencies through a dedicated one-shot callback.
//! Keeping the wire conversion here means cache invalidation and the UI use
//! the same typed representation without depending on logger activities.

use std::path::PathBuf;
use std::sync::Arc;

/// A filesystem or environment operation observed during Nix evaluation.
///
/// These operations are used for cache invalidation and dependency tracking.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EvalOp {
    /// Copied a source path to the Nix store.
    CopiedSource { source: PathBuf, target: PathBuf },
    /// Filtered a source tree and copied it to the Nix store.
    FilteredSource { source: PathBuf, target: PathBuf },
    /// Evaluated a Nix file.
    EvaluatedFile { source: PathBuf, cached: bool },
    /// Read a file's contents with `builtins.readFile`.
    ReadFile { source: PathBuf },
    /// List a directory's contents with `builtins.readDir`.
    ReadDir { source: PathBuf },
    /// Read a file type with `builtins.readFileType`.
    ReadFileType { source: PathBuf },
    /// Hashed a file with `builtins.hashFile`.
    HashFile { source: PathBuf, algorithm: String },
    /// Read an environment variable with `builtins.getEnv`.
    GetEnv { name: String },
    /// Check that a file exists with `builtins.pathExists`.
    PathExists { source: PathBuf },
}

/// Exact state captured at the moment an evaluation input was consumed.
///
/// Primops use this when re-reading the input later could associate a stale
/// evaluation result with newer file or environment state.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EvalInputState {
    File {
        path: PathBuf,
        content_sha256: Option<String>,
    },
    Env {
        name: String,
        content_sha256: Option<String>,
    },
}

impl EvalInputState {
    pub fn operation(&self) -> EvalOp {
        match self {
            Self::File { path, .. } => EvalOp::ReadFile {
                source: path.clone(),
            },
            Self::Env { name, .. } => EvalOp::GetEnv { name: name.clone() },
        }
    }
}

/// Convert to the activity event type for serialization.
impl From<EvalOp> for devenv_activity::EvalOp {
    fn from(op: EvalOp) -> Self {
        match op {
            EvalOp::CopiedSource { source, target } => {
                devenv_activity::EvalOp::CopiedSource { source, target }
            }
            EvalOp::FilteredSource { source, target } => {
                devenv_activity::EvalOp::FilteredSource { source, target }
            }
            EvalOp::EvaluatedFile { source, cached } => {
                devenv_activity::EvalOp::EvaluatedFile { source, cached }
            }
            EvalOp::ReadFile { source } => devenv_activity::EvalOp::ReadFile { source },
            EvalOp::ReadDir { source } => devenv_activity::EvalOp::ReadDir { source },
            EvalOp::ReadFileType { source } => devenv_activity::EvalOp::ReadFileType { source },
            EvalOp::HashFile { source, algorithm } => {
                devenv_activity::EvalOp::HashFile { source, algorithm }
            }
            EvalOp::GetEnv { name } => devenv_activity::EvalOp::GetEnv { name },
            EvalOp::PathExists { source } => devenv_activity::EvalOp::PathExists { source },
        }
    }
}

impl EvalOp {
    /// Extract an operation from the dedicated one-shot evaluator effect
    /// callback. This is the canonical wire conversion used by the Nix FFI.
    pub fn from_effect(kind: &str, subject: &str, detail: Option<&str>) -> Option<Self> {
        let path = || PathBuf::from(subject);

        match (kind, detail) {
            ("copy-source", Some(target)) => Some(EvalOp::CopiedSource {
                source: path(),
                target: PathBuf::from(target),
            }),
            ("filter-source", Some(target)) => Some(EvalOp::FilteredSource {
                source: path(),
                target: PathBuf::from(target),
            }),
            ("evaluated-file", Some("cached")) => Some(EvalOp::EvaluatedFile {
                source: path(),
                cached: true,
            }),
            ("evaluated-file", Some("uncached")) => Some(EvalOp::EvaluatedFile {
                source: path(),
                cached: false,
            }),
            ("read-file", None) => Some(EvalOp::ReadFile { source: path() }),
            ("read-dir", None) => Some(EvalOp::ReadDir { source: path() }),
            ("read-file-type", None) => Some(EvalOp::ReadFileType { source: path() }),
            ("hash-file", Some(algorithm)) if !algorithm.is_empty() => Some(EvalOp::HashFile {
                source: path(),
                algorithm: algorithm.to_owned(),
            }),
            ("get-env", None) => Some(EvalOp::GetEnv {
                name: subject.to_owned(),
            }),
            ("path-exists", None) => Some(EvalOp::PathExists { source: path() }),
            _ => None,
        }
    }
}

/// Observer trait for receiving evaluation operations.
///
/// Implementations can be registered with `NixLogBridge` to receive file and
/// environment dependencies during evaluation.
pub trait OpObserver: Send + Sync + 'static {
    /// Called when an operation is observed during evaluation.
    fn record(&self, op: EvalOp);

    /// Record both an input identity and the state actually consumed.
    fn record_input_state(&self, input: EvalInputState) {
        self.record(input.operation());
    }
}

/// Wrapper to allow `Arc<dyn OpObserver>` to implement `OpObserver`.
impl OpObserver for Arc<dyn OpObserver> {
    fn record(&self, op: EvalOp) {
        (**self).record(op);
    }

    fn record_input_state(&self, input: EvalInputState) {
        (**self).record_input_state(input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_eval_effects() {
        let cases = [
            (
                "copy-source",
                "/source",
                Some("/nix/store/source"),
                EvalOp::CopiedSource {
                    source: "/source".into(),
                    target: "/nix/store/source".into(),
                },
            ),
            (
                "filter-source",
                "/source",
                Some("/nix/store/source"),
                EvalOp::FilteredSource {
                    source: "/source".into(),
                    target: "/nix/store/source".into(),
                },
            ),
            (
                "evaluated-file",
                "/default.nix",
                Some("cached"),
                EvalOp::EvaluatedFile {
                    source: "/default.nix".into(),
                    cached: true,
                },
            ),
            (
                "evaluated-file",
                "/default.nix",
                Some("uncached"),
                EvalOp::EvaluatedFile {
                    source: "/default.nix".into(),
                    cached: false,
                },
            ),
            (
                "read-file",
                "/file",
                None,
                EvalOp::ReadFile {
                    source: "/file".into(),
                },
            ),
            (
                "read-dir",
                "/dir",
                None,
                EvalOp::ReadDir {
                    source: "/dir".into(),
                },
            ),
            (
                "read-file-type",
                "/file",
                None,
                EvalOp::ReadFileType {
                    source: "/file".into(),
                },
            ),
            (
                "hash-file",
                "/file",
                Some("sha256"),
                EvalOp::HashFile {
                    source: "/file".into(),
                    algorithm: "sha256".into(),
                },
            ),
            (
                "get-env",
                "SOME_ENV",
                None,
                EvalOp::GetEnv {
                    name: "SOME_ENV".into(),
                },
            ),
            (
                "path-exists",
                "/file",
                None,
                EvalOp::PathExists {
                    source: "/file".into(),
                },
            ),
        ];

        for (kind, subject, detail, expected) in cases {
            assert_eq!(EvalOp::from_effect(kind, subject, detail), Some(expected));
        }
    }

    #[test]
    fn rejects_unknown_or_malformed_effects() {
        assert_eq!(EvalOp::from_effect("unknown", "/file", None), None);
        assert_eq!(
            EvalOp::from_effect("read-file", "/file", Some("unexpected")),
            None
        );
        assert_eq!(EvalOp::from_effect("copy-source", "/source", None), None);
        assert_eq!(
            EvalOp::from_effect("evaluated-file", "/file", Some("old")),
            None
        );
        assert_eq!(EvalOp::from_effect("hash-file", "/file", Some("")), None);
    }
}
