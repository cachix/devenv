//! Task Server Protocol type definitions
//!
//! This module defines the shared types used by both the server and client
//! implementations of the Task Server Protocol.

use serde::{Deserialize, Serialize};

/// Represents a task in the Task Server Protocol
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    /// Unique identifier for the task
    pub name: String,

    /// Optional description of what the task does
    #[serde(default)]
    pub description: String,

    /// List of task names that must run before this task
    #[serde(default)]
    pub after: Vec<String>,

    /// List of task names that must run after this task
    #[serde(default)]
    pub before: Vec<String>,

    /// Input schema for the task
    #[serde(default)]
    pub input: serde_json::Value,
}

/// Task execution status
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum TaskStatus {
    /// Task is waiting to be executed
    Pending,

    /// Task is currently running
    Running,

    /// Task completed successfully
    Success,

    /// Task failed during execution
    Failed {
        /// Error message describing why the task failed
        error: String,
    },

    /// Task was skipped (due to cache or conditional execution)
    Skipped {
        /// Reason why the task was skipped
        reason: String,
    },
}

/// Task execution request
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecuteTaskRequest {
    /// Name of the task to execute
    pub name: String,

    /// Input parameters for the task
    #[serde(default)]
    pub input: serde_json::Value,

    /// Whether to force execution even if task would be skipped
    #[serde(default)]
    pub force: bool,
}

/// Task execution result
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskResult {
    /// Name of the task that was executed
    pub name: String,

    /// Final status of the task
    pub status: TaskStatus,

    /// Task output data
    #[serde(default)]
    pub output: serde_json::Value,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Log entry from a task
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LogEntry {
    /// Name of the task that produced this log
    pub task_name: String,

    /// Log level (info, warn, error, debug)
    pub level: String,

    /// Log message content
    pub message: String,

    /// Timestamp when the log was generated (ISO 8601 format)
    pub timestamp: String,
}

/// JSON-RPC method names defined by the Task Server Protocol
pub mod methods {
    /// List all available tasks
    pub const LIST_TASKS: &str = "tsp_listTasks";

    /// Get detailed information about a specific task
    pub const GET_TASK: &str = "tsp_getTask";

    /// Execute a task
    pub const EXECUTE_TASK: &str = "tsp_executeTask";

    /// Get the current status of a task
    pub const GET_TASK_STATUS: &str = "tsp_getTaskStatus";

    /// Stream logs from a task (notification)
    pub const TASK_LOG: &str = "tsp_taskLog";

    /// Notification for task status changes
    pub const TASK_STATUS_CHANGED: &str = "tsp_taskStatusChanged";
}
