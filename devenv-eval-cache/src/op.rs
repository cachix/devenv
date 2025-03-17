use crate::internal_log::InternalLog;

use regex::Regex;
use std::path::PathBuf;

/// A sum-type of filesystem operations that we can extract from the Nix logs.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
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
    /// Check that a file exists with 'builtins.pathExists'.
    PathExists { source: PathBuf },
    /// Used a tracked devenv string path.
    TrackedPath { source: PathBuf },
}

impl Op {
    /// Extract an `Op` from a `InternalLog`.
    pub fn from_internal_log(log: &InternalLog) -> Option<Self> {
        lazy_static::lazy_static! {
            static ref EVALUATED_FILE: Regex =
               Regex::new("^evaluating file '(?P<source>.*)'$").expect("invalid regex");
            static ref COPIED_SOURCE: Regex =
                Regex::new("^copied source '(?P<source>.*)' -> '(?P<target>.*)'$").expect("invalid regex");
            static ref READ_FILE: Regex =
                Regex::new("^devenv readFile: '(?P<source>.*)'$").expect("invalid regex");
            static ref READ_DIR: Regex =
                Regex::new("^devenv readDir: '(?P<source>.*)'$").expect("invalid regex");
            static ref GET_ENV: Regex =
                Regex::new("^devenv getEnv: '(?P<name>.*)'$").expect("invalid regex");
            static ref PATH_EXISTS: Regex =
                Regex::new("^devenv pathExists: '(?P<source>.*)'$").expect("invalid regex");
            static ref TRACKED_PATH: Regex =
                Regex::new("^trace: devenv path: '(?P<source>.*)'$").expect("invalid regex");
        }

        match log {
            InternalLog::Msg { msg, .. } => {
                if let Some(matches) = COPIED_SOURCE.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    let target = PathBuf::from(&matches["target"]);
                    Some(Op::CopiedSource { source, target })
                } else if let Some(matches) = EVALUATED_FILE.captures(msg) {
                    let mut source = PathBuf::from(&matches["source"]);
                    // If the evaluated file is a directory, we assume that the file is `default.nix`.
                    if source.is_dir() {
                        source.push("default.nix");
                    }
                    Some(Op::EvaluatedFile { source })
                } else if let Some(matches) = READ_FILE.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(Op::ReadFile { source })
                } else if let Some(matches) = READ_DIR.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(Op::ReadDir { source })
                } else if let Some(matches) = GET_ENV.captures(msg) {
                    let name = matches["name"].to_string();
                    Some(Op::GetEnv { name })
                } else if let Some(matches) = PATH_EXISTS.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(Op::PathExists { source })
                } else if let Some(matches) = TRACKED_PATH.captures(msg) {
                    let source = PathBuf::from(&matches["source"]);
                    Some(Op::TrackedPath { source })
                } else {
                    None
                }
            }
            _ => None,
        }
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
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::CopiedSource {
                source: PathBuf::from("/path/to/source"),
                target: PathBuf::from("/path/to/target"),
            })
        );
    }

    #[test]
    fn test_evaluated_file() {
        let log = create_log("evaluating file '/path/to/file'");
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::EvaluatedFile {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_read_file() {
        let log = create_log("devenv readFile: '/path/to/file'");
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::ReadFile {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_read_dir() {
        let log = create_log("devenv readDir: '/path/to/dir'");
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::ReadDir {
                source: PathBuf::from("/path/to/dir"),
            })
        );
    }

    #[test]
    fn test_get_env() {
        let log = create_log("devenv getEnv: 'SOME_ENV'");
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::GetEnv {
                name: "SOME_ENV".to_string(),
            })
        );
    }

    #[test]
    fn test_path_exists() {
        let log = create_log("devenv pathExists: '/path/to/file'");
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::PathExists {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_tracked_path() {
        let log = create_log("trace: devenv path: '/path/to/file'");
        let op = Op::from_internal_log(&log);
        assert_eq!(
            op,
            Some(Op::TrackedPath {
                source: PathBuf::from("/path/to/file"),
            })
        );
    }

    #[test]
    fn test_unmatched_log() {
        let log = create_log("some unrelated message");
        let op = Op::from_internal_log(&log);
        assert_eq!(op, None);
    }

    #[test]
    fn test_non_msg_log() {
        let log = InternalLog::Stop { id: 1 };
        let op = Op::from_internal_log(&log);
        assert_eq!(op, None);
    }
}
