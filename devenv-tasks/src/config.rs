use crate::SudoContext;
use crate::error::Error;
use crate::types::{DependencyKind, DependencySpec, TaskType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskConfig {
    pub name: String,
    #[serde(default)]
    pub r#type: TaskType,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub exec_if_modified: Vec<String>,
    #[serde(default)]
    pub inputs: Option<serde_json::Value>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub show_output: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// Run only the specified task without dependencies
    Single,
    /// Run the specified task and all tasks that depend on it (downstream tasks)
    After,
    /// Run all dependency tasks first, then the specified task (upstream tasks)
    Before,
    #[default]
    /// Run the complete dependency graph (upstream and downstream tasks)
    All,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub tasks: Vec<TaskConfig>,
    pub roots: Vec<String>,
    pub run_mode: RunMode,
    #[serde(skip)]
    pub sudo_context: Option<std::sync::Arc<SudoContext>>,
}

impl TryFrom<serde_json::Value> for Config {
    type Error = serde_json::Error;

    fn try_from(json: serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(json)
    }
}

/// Parse a dependency string with optional suffix notation
///
/// Supported formats:
/// - "task" -> DependencySpec { name: "task", kind: Ready } (default)
/// - "task@ready" -> DependencySpec { name: "task", kind: Ready }
/// - "task@complete" -> DependencySpec { name: "task", kind: Complete }
///
/// Returns an error if the suffix is invalid or '@' appears in the middle of the name
pub fn parse_dependency(dep: &str) -> Result<DependencySpec, Error> {
    if let Some((name, suffix)) = dep.rsplit_once('@') {
        // Validate that name doesn't contain '@' (only one '@' allowed at the end)
        if name.contains('@') {
            return Err(Error::InvalidDependency(format!(
                "Invalid dependency '{}': multiple '@' characters not allowed",
                dep
            )));
        }

        let kind = match suffix {
            "ready" => DependencyKind::Ready,
            "complete" => DependencyKind::Complete,
            _ => {
                return Err(Error::InvalidDependency(format!(
                    "Invalid dependency '{}': suffix must be '@ready' or '@complete', got '@{}'",
                    dep, suffix
                )));
            }
        };

        Ok(DependencySpec {
            name: name.to_string(),
            kind,
        })
    } else {
        // No suffix, default to Ready
        Ok(DependencySpec {
            name: dep.to_string(),
            kind: DependencyKind::Ready,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dependency_no_suffix() {
        let spec = parse_dependency("postgres").unwrap();
        assert_eq!(spec.name, "postgres");
        assert_eq!(spec.kind, DependencyKind::Ready);
    }

    #[test]
    fn test_parse_dependency_ready_suffix() {
        let spec = parse_dependency("postgres@ready").unwrap();
        assert_eq!(spec.name, "postgres");
        assert_eq!(spec.kind, DependencyKind::Ready);
    }

    #[test]
    fn test_parse_dependency_complete_suffix() {
        let spec = parse_dependency("postgres@complete").unwrap();
        assert_eq!(spec.name, "postgres");
        assert_eq!(spec.kind, DependencyKind::Complete);
    }

    #[test]
    fn test_parse_dependency_invalid_suffix() {
        let result = parse_dependency("postgres@invalid");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must be '@ready' or '@complete'")
        );
    }

    #[test]
    fn test_parse_dependency_multiple_at() {
        let result = parse_dependency("foo@bar@ready");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("multiple '@' characters")
        );
    }

    #[test]
    fn test_parse_dependency_with_namespace() {
        let spec = parse_dependency("devenv:processes:postgres@complete").unwrap();
        assert_eq!(spec.name, "devenv:processes:postgres");
        assert_eq!(spec.kind, DependencyKind::Complete);
    }
}
