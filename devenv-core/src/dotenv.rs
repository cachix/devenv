//! Shared dotenv loading and evaluation-cache tracking.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use dotenv::{EnvLoader, EnvSequence};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use sha2::{Digest, Sha256};

use crate::eval_op::{EvalInputState, OpObserver};

/// Transient file state used to reject dotenv changes during a load.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DotenvFileSpec {
    path: PathBuf,
    state: DotenvFileState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DotenvFileState {
    Missing,
    Present { sha256: String },
}

/// Load dotenv files with the same parser used by both the CLI runtime path
/// and the Nix primop.
pub fn load_dotenv(paths: &[PathBuf], substitution: bool) -> Result<BTreeMap<String, String>> {
    load_dotenv_inner(paths, substitution).map(|(variables, _, _)| variables)
}

/// Load dotenv files and report ordinary file/env observations to the
/// evaluation input tracker. This prevents cached Nix values from surviving a
/// dotenv or substitution-source edit.
pub fn load_dotenv_tracked(
    paths: &[PathBuf],
    substitution: bool,
    observer: &dyn OpObserver,
) -> Result<BTreeMap<String, String>> {
    let before = capture_files(paths.iter().map(PathBuf::as_path))?;
    let (variables, substitution_dependencies, substitutions) =
        load_dotenv_inner(paths, substitution)?;
    let after = capture_files(paths.iter().map(PathBuf::as_path))?;

    if before != after {
        bail!("A dotenv file changed while it was being loaded; retry the command");
    }

    for file in before {
        observer.record_input_state(EvalInputState::File {
            path: file.path,
            content_sha256: match file.state {
                DotenvFileState::Missing => None,
                DotenvFileState::Present { sha256 } => Some(sha256),
            },
        });
    }
    for name in substitution_dependencies {
        let content_sha256 = substitutions
            .as_ref()
            .and_then(|values| values.get(&name))
            .map(|value| hex::encode(Sha256::digest(value.as_bytes())));
        observer.record_input_state(EvalInputState::Env {
            name,
            content_sha256,
        });
    }
    Ok(variables)
}

fn load_dotenv_inner(
    paths: &[PathBuf],
    substitution: bool,
) -> Result<(
    BTreeMap<String, String>,
    Vec<String>,
    Option<BTreeMap<String, String>>,
)> {
    if paths.is_empty() {
        return Ok((BTreeMap::new(), Vec::new(), None));
    }

    let substitutions = substitution.then(inherited_substitutions);
    let loader = EnvLoader::with_paths(paths)
        .sequence(EnvSequence::InputOnly)
        .required(false)
        .substitution(substitution)
        .substitutions(substitutions.as_ref().into_iter().flat_map(|values| {
            values
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
        }));
    let (loaded, dependencies) = if substitution {
        loader.load_with_substitution_dependencies()
    } else {
        loader
            .load()
            .map(|variables| (variables, Default::default()))
    }
    .into_diagnostic()
    .wrap_err("Failed to load dotenv files")?;

    let mut variables = BTreeMap::new();
    for (name, value) in loaded {
        if !is_valid_env_name(&name) {
            bail!(
                "Invalid environment variable name '{name}' in dotenv file: names must match [A-Za-z_][A-Za-z0-9_]*"
            );
        }
        variables.insert(name, value);
    }

    let mut dependencies: Vec<_> = dependencies.into_iter().collect();
    dependencies.sort();
    Ok((variables, dependencies, substitutions))
}

fn capture_files<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Result<Vec<DotenvFileSpec>> {
    paths
        .into_iter()
        .map(|path| {
            let state = match fs::read(path) {
                Ok(contents) => DotenvFileState::Present {
                    sha256: hex::encode(Sha256::digest(contents)),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    DotenvFileState::Missing
                }
                Err(error) => {
                    return Err(error).into_diagnostic().wrap_err_with(|| {
                        format!("Failed to hash dotenv file '{}'", path.display())
                    });
                }
            };
            Ok(DotenvFileSpec {
                path: path.to_path_buf(),
                state,
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn inherited_substitutions() -> BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_op::EvalOp;
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingObserver(Mutex<Vec<EvalOp>>);

    impl OpObserver for RecordingObserver {
        fn record(&self, op: EvalOp) {
            self.0.lock().unwrap().push(op);
        }
    }

    #[test]
    fn tracked_load_records_present_and_missing_files() {
        let root = TempDir::new().unwrap();
        let present = root.path().join(".env");
        let missing = root.path().join(".env.local");
        fs::write(&present, "VALUE=one\n").unwrap();

        let observer = RecordingObserver::default();
        load_dotenv_tracked(&[present.clone(), missing.clone()], false, &observer).unwrap();
        let observations = observer.0.lock().unwrap();
        assert!(observations.contains(&EvalOp::ReadFile { source: present }));
        assert!(observations.contains(&EvalOp::ReadFile { source: missing }));
    }

    #[test]
    fn tracked_load_records_substitution_names_without_values() {
        let root = TempDir::new().unwrap();
        let path = root.path().join(".env");
        fs::write(&path, "VALUE=$HOME\n").unwrap();

        let observer = RecordingObserver::default();
        load_dotenv_tracked(&[path], true, &observer).unwrap();
        assert!(observer.0.lock().unwrap().contains(&EvalOp::GetEnv {
            name: "HOME".into()
        }));
    }
}
