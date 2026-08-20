//! Shared dotenv loading and evaluation-cache tracking.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dotenv::{EnvLoader, EnvSequence};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::resource::{ReplayError, ReplayableResource};

/// The state of dotenv inputs used by a Nix evaluation.
///
/// Only paths and hashes are persisted in the evaluation cache. Dotenv and
/// inherited environment values are deliberately omitted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DotenvSpec {
    pub files: Vec<DotenvFileSpec>,
    pub substitution_environment: Option<DotenvSubstitutionSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DotenvSubstitutionSpec {
    /// Names consulted during interpolation. Values are represented only by
    /// the combined hash below and are never persisted.
    pub variables: Vec<String>,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DotenvFileSpec {
    pub path: PathBuf,
    pub state: DotenvFileState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DotenvFileState {
    Missing,
    Present { sha256: String },
}

/// Tracks dotenv inputs read by the Nix primop.
#[derive(Debug, Default)]
pub struct DotenvTracker {
    spec: Mutex<DotenvSpec>,
}

impl DotenvTracker {
    fn record(&self, spec: DotenvSpec) {
        *self.spec.lock().unwrap() = spec;
    }
}

impl ReplayableResource for DotenvTracker {
    type Spec = DotenvSpec;
    const TYPE_ID: &'static str = "dotenv";

    fn snapshot(&self) -> Self::Spec {
        self.spec.lock().unwrap().clone()
    }

    fn replay(&self, spec: &Self::Spec) -> std::result::Result<(), ReplayError> {
        let current = DotenvSpec {
            files: capture_files(spec.files.iter().map(|file| file.path.as_path()))
                .map_err(|error| ReplayError::Unavailable(error.to_string()))?,
            substitution_environment: spec
                .substitution_environment
                .as_ref()
                .map(|environment| capture_substitution_environment(&environment.variables)),
        };

        if &current != spec {
            return Err(ReplayError::Unavailable(
                "dotenv inputs changed since the cached evaluation".to_string(),
            ));
        }

        self.record(spec.clone());
        Ok(())
    }

    fn clear(&self) {
        self.record(DotenvSpec::default());
    }
}

/// Load dotenv files with the same parser used by both the CLI runtime path
/// and the Nix primop.
pub fn load_dotenv(paths: &[PathBuf], substitution: bool) -> Result<BTreeMap<String, String>> {
    load_dotenv_inner(paths, substitution).map(|(variables, _)| variables)
}

/// Load dotenv files and record their dependency hashes for evaluation-cache
/// replay. This prevents cached Nix values from surviving a dotenv edit.
pub fn load_dotenv_tracked(
    paths: &[PathBuf],
    substitution: bool,
    tracker: &DotenvTracker,
) -> Result<BTreeMap<String, String>> {
    let before = capture_files(paths.iter().map(PathBuf::as_path))?;
    let (variables, substitution_environment) = load_dotenv_inner(paths, substitution)?;
    let after = capture_files(paths.iter().map(PathBuf::as_path))?;

    if before != after {
        bail!("A dotenv file changed while it was being loaded; retry the command");
    }

    // Use the exact inherited environment snapshot supplied to dotenv-ng,
    // rather than a second capture that could race with evaluation.
    tracker.record(DotenvSpec {
        files: before,
        substitution_environment,
    });
    Ok(variables)
}

fn load_dotenv_inner(
    paths: &[PathBuf],
    substitution: bool,
) -> Result<(BTreeMap<String, String>, Option<DotenvSubstitutionSpec>)> {
    if paths.is_empty() {
        return Ok((BTreeMap::new(), None));
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

    let substitution_environment = substitutions
        .as_ref()
        .map(|values| substitution_environment_spec(values, dependencies));
    Ok((variables, substitution_environment))
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

fn capture_substitution_environment(variables: &[String]) -> DotenvSubstitutionSpec {
    substitution_environment_spec(&inherited_substitutions(), variables.iter().cloned())
}

fn substitution_environment_spec(
    substitutions: &BTreeMap<String, String>,
    variables: impl IntoIterator<Item = String>,
) -> DotenvSubstitutionSpec {
    let mut variables: Vec<_> = variables.into_iter().collect();
    variables.sort();

    let mut hasher = Sha256::new();
    for name in &variables {
        hasher.update(name.len().to_le_bytes());
        hasher.update(name.as_bytes());
        match substitutions.get(name) {
            Some(value) => {
                hasher.update([1]);
                hasher.update(value.len().to_le_bytes());
                hasher.update(value.as_bytes());
            }
            None => hasher.update([0]),
        }
    }
    DotenvSubstitutionSpec {
        variables,
        sha256: hex::encode(hasher.finalize()),
    }
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
    use tempfile::TempDir;

    #[test]
    fn tracker_rejects_changed_and_new_files() {
        let root = TempDir::new().unwrap();
        let present = root.path().join(".env");
        let missing = root.path().join(".env.local");
        fs::write(&present, "VALUE=one\n").unwrap();

        let tracker = DotenvTracker::default();
        load_dotenv_tracked(&[present.clone(), missing.clone()], false, &tracker).unwrap();
        let spec = tracker.snapshot();
        assert!(tracker.replay(&spec).is_ok());

        fs::write(&present, "VALUE=two\n").unwrap();
        assert!(tracker.replay(&spec).is_err());

        fs::write(&present, "VALUE=one\n").unwrap();
        fs::write(&missing, "LOCAL=yes\n").unwrap();
        assert!(tracker.replay(&spec).is_err());
    }

    #[test]
    fn tracker_spec_does_not_contain_values() {
        let root = TempDir::new().unwrap();
        let path = root.path().join(".env");
        fs::write(&path, "SECRET=must-not-be-cached\n").unwrap();

        let tracker = DotenvTracker::default();
        load_dotenv_tracked(&[path], false, &tracker).unwrap();
        let json = serde_json::to_string(&tracker.snapshot()).unwrap();

        assert!(!json.contains("must-not-be-cached"));
        assert!(!json.contains("SECRET"));
    }
}
