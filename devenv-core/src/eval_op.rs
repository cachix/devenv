//! Evaluation operation types and structured Nix effect parsing.
//!
//! Nix emits evaluation dependencies as `eval-effect` activities. Keeping the
//! wire conversion here means cache invalidation and the UI use the same typed
//! representation without depending on human-readable log messages.

use crate::internal_log::{ActivityType, Field};
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
    EvaluatedFile { source: PathBuf },
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
            EvalOp::EvaluatedFile { source } => devenv_activity::EvalOp::EvaluatedFile { source },
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
    /// Extract an `EvalOp` from a structured Nix `eval-effect` activity.
    ///
    /// The wire schema is `[kind, subject, optional detail]`; every field is a
    /// string. `copy-source` and `filter-source` use detail for their store
    /// target, `evaluated-file` uses `cached` or `uncached`, and `hash-file`
    /// carries the hash algorithm. Unknown kinds and malformed payloads are
    /// intentionally ignored so dependency tracking never guesses from a
    /// changing protocol.
    pub fn from_activity(typ: ActivityType, fields: &[Field]) -> Option<Self> {
        if typ != ActivityType::EvalEffect {
            return None;
        }

        let [Field::String(kind), Field::String(subject), rest @ ..] = fields else {
            return None;
        };
        let path = || PathBuf::from(subject);

        match (kind.as_str(), rest) {
            ("copy-source", [Field::String(target)]) => Some(EvalOp::CopiedSource {
                source: path(),
                target: PathBuf::from(target),
            }),
            ("filter-source", [Field::String(target)]) => Some(EvalOp::FilteredSource {
                source: path(),
                target: PathBuf::from(target),
            }),
            ("evaluated-file", [Field::String(state)])
                if matches!(state.as_str(), "cached" | "uncached") =>
            {
                Some(EvalOp::EvaluatedFile { source: path() })
            }
            ("read-file", []) => Some(EvalOp::ReadFile { source: path() }),
            ("read-dir", []) => Some(EvalOp::ReadDir { source: path() }),
            ("read-file-type", []) => Some(EvalOp::ReadFileType { source: path() }),
            ("hash-file", [Field::String(algorithm)]) if !algorithm.is_empty() => {
                Some(EvalOp::HashFile {
                    source: path(),
                    algorithm: algorithm.clone(),
                })
            }
            ("get-env", []) => Some(EvalOp::GetEnv {
                name: subject.clone(),
            }),
            ("path-exists", []) => Some(EvalOp::PathExists { source: path() }),
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
}

/// Wrapper to allow `Arc<dyn OpObserver>` to implement `OpObserver`.
impl OpObserver for Arc<dyn OpObserver> {
    fn record(&self, op: EvalOp) {
        (**self).record(op);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(fields: Vec<Field>) -> Option<EvalOp> {
        EvalOp::from_activity(ActivityType::EvalEffect, &fields)
    }

    fn strings(fields: &[&str]) -> Vec<Field> {
        fields
            .iter()
            .map(|field| Field::String((*field).to_string()))
            .collect()
    }

    #[test]
    fn parses_all_eval_effects() {
        let cases = [
            (
                strings(&["copy-source", "/source", "/nix/store/source"]),
                EvalOp::CopiedSource {
                    source: "/source".into(),
                    target: "/nix/store/source".into(),
                },
            ),
            (
                strings(&["filter-source", "/source", "/nix/store/source"]),
                EvalOp::FilteredSource {
                    source: "/source".into(),
                    target: "/nix/store/source".into(),
                },
            ),
            (
                strings(&["evaluated-file", "/default.nix", "cached"]),
                EvalOp::EvaluatedFile {
                    source: "/default.nix".into(),
                },
            ),
            (
                strings(&["evaluated-file", "/default.nix", "uncached"]),
                EvalOp::EvaluatedFile {
                    source: "/default.nix".into(),
                },
            ),
            (
                strings(&["read-file", "/file"]),
                EvalOp::ReadFile {
                    source: "/file".into(),
                },
            ),
            (
                strings(&["read-dir", "/dir"]),
                EvalOp::ReadDir {
                    source: "/dir".into(),
                },
            ),
            (
                strings(&["read-file-type", "/file"]),
                EvalOp::ReadFileType {
                    source: "/file".into(),
                },
            ),
            (
                strings(&["hash-file", "/file", "sha256"]),
                EvalOp::HashFile {
                    source: "/file".into(),
                    algorithm: "sha256".into(),
                },
            ),
            (
                strings(&["get-env", "SOME_ENV"]),
                EvalOp::GetEnv {
                    name: "SOME_ENV".into(),
                },
            ),
            (
                strings(&["path-exists", "/file"]),
                EvalOp::PathExists {
                    source: "/file".into(),
                },
            ),
        ];

        for (fields, expected) in cases {
            assert_eq!(effect(fields), Some(expected));
        }
    }

    #[test]
    fn rejects_non_effect_activities_and_malformed_payloads() {
        assert_eq!(
            EvalOp::from_activity(ActivityType::Build, &strings(&["read-file", "/file"])),
            None
        );
        assert_eq!(effect(strings(&["unknown", "/file"])), None);
        assert_eq!(effect(strings(&["read-file", "/file", "unexpected"])), None);
        assert_eq!(effect(strings(&["copy-source", "/source"])), None);
        assert_eq!(effect(strings(&["evaluated-file", "/file", "old"])), None);
        assert_eq!(effect(strings(&["hash-file", "/file", ""])), None);
        assert_eq!(
            effect(vec![Field::String("read-file".into()), Field::Int(1),]),
            None
        );
    }
}
