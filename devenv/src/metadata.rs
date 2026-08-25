//! Structured metadata about a devenv environment.

use std::collections::BTreeMap;
use std::path::Path;

use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix_flake_lock::{AttrValue, Edge, LockFile};

/// Named environment information contributed by devenv modules.
pub type InfoSections = BTreeMap<String, Vec<String>>;

/// Metadata about a devenv environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Metadata {
    /// Root inputs from `devenv.lock`, or `None` when no lock file exists.
    pub inputs: Option<Vec<InputMetadata>>,
    /// Information contributed through `config.infoSections`.
    pub info_sections: InfoSections,
}

/// Metadata about one root input in `devenv.lock`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputMetadata {
    pub name: String,
    pub source: InputSource,
}

/// How a root input is resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputSource {
    /// The input follows another input path.
    Follows(Vec<String>),
    /// The input resolves to a locked fetcher attribute set.
    Locked(BTreeMap<String, InputAttribute>),
}

/// An owned value from a locked fetcher attribute set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputAttribute {
    String(String),
    Integer(u64),
    Bool(bool),
}

impl InputAttribute {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            Self::Integer(_) | Self::Bool(_) => None,
        }
    }

    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::String(_) | Self::Integer(_) => None,
        }
    }
}

impl From<&AttrValue<'_>> for InputAttribute {
    fn from(value: &AttrValue<'_>) -> Self {
        match value {
            AttrValue::String(value) => Self::String(value.to_string()),
            AttrValue::Integer(value) => Self::Integer(*value),
            AttrValue::Bool(value) => Self::Bool(*value),
        }
    }
}

pub(crate) fn load_inputs(lock_path: &Path) -> Result<Option<Vec<InputMetadata>>> {
    if !lock_path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(lock_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read {}", lock_path.display()))?;
    let lock = LockFile::parse(&bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to parse {}", lock_path.display()))?;
    lock.validate()
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to validate {}", lock_path.display()))?;

    let inputs = lock
        .root()
        .inputs()
        .iter()
        .map(|input| {
            let source = match input.edge() {
                Edge::Follows(path) => {
                    InputSource::Follows(path.iter().map(ToOwned::to_owned).collect())
                }
                Edge::Node(node_id) => {
                    let locked = lock
                        .node(*node_id)
                        .and_then(|node| node.locked())
                        .ok_or_else(|| {
                            miette!("input {:?} does not point to a locked node", input.name())
                        })?;
                    InputSource::Locked(
                        locked
                            .locked()
                            .iter()
                            .map(|attr| {
                                (attr.name().to_string(), InputAttribute::from(attr.value()))
                            })
                            .collect(),
                    )
                }
            };
            Ok(InputMetadata {
                name: input.name().to_string(),
                source,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Some(inputs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const LOCK: &str = r#"{
      "nodes": {
        "github": {
          "locked": {
            "narHash": "sha256-a/b+c=",
            "owner": "example",
            "repo": "project",
            "rev": "0123456789abcdef0123456789abcdef01234567",
            "type": "github"
          },
          "original": { "owner": "example", "repo": "project", "type": "github" }
        },
        "root": {
          "inputs": {
            "follows": ["github"],
            "github": "github"
          }
        }
      },
      "root": "root",
      "version": 7
    }"#;

    #[test]
    fn loads_root_inputs_without_nix() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("custom.lock");
        std::fs::write(&lock_path, LOCK).unwrap();

        let inputs = load_inputs(&lock_path).unwrap().unwrap();
        assert_eq!(inputs[0].name, "follows");
        assert_eq!(
            inputs[0].source,
            InputSource::Follows(vec!["github".to_string()])
        );
        assert_eq!(inputs[1].name, "github");
        let InputSource::Locked(attributes) = &inputs[1].source else {
            panic!("github should be a locked input");
        };
        assert_eq!(
            attributes.get("owner").and_then(InputAttribute::as_str),
            Some("example")
        );
    }

    #[test]
    fn reports_a_missing_lock_file_as_none() {
        let temp = TempDir::new().unwrap();
        assert_eq!(load_inputs(&temp.path().join("custom.lock")).unwrap(), None);
    }
}
