use serde::{Deserialize, Serialize};
use crate::SudoContext;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskConfig {
    pub name: String,
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
    pub sudo_context: Option<SudoContext>,
}

impl TryFrom<serde_json::Value> for Config {
    type Error = serde_json::Error;

    fn try_from(json: serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(json)
    }
}
