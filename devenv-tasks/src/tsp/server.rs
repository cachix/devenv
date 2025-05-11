//! Task Server Protocol server implementation
//!
//! This module provides the server implementation for the Task Server Protocol,
//! which allows task execution and management over JSON-RPC via Unix sockets.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use karyon_jsonrpc::{
    error::RPCError as JsonRpcError,
    error::RPCResult,
    server::service::{RPCMethod, RPCService},
    server::ServerBuilder,
};
use serde_json::{json, Value};
use tokio::fs;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info};

use super::protocol::{methods, ExecuteTaskRequest, LogEntry, Task, TaskResult, TaskStatus};

/// Error type for Task Server operations
#[derive(Debug, thiserror::Error)]
pub enum TspError {
    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Task execution error: {0}")]
    ExecutionError(String),

    #[error("Task already running: {0}")]
    AlreadyRunning(String),
}

impl From<TspError> for JsonRpcError {
    fn from(err: TspError) -> Self {
        match err {
            TspError::TaskNotFound(msg) => {
                JsonRpcError::CustomError(-32000, format!("Task not found: {}", msg))
            }
            TspError::InvalidParameters(msg) => {
                JsonRpcError::InvalidParams(format!("Invalid parameters: {}", msg))
            }
            TspError::ExecutionError(msg) => {
                JsonRpcError::CustomError(-32001, format!("Task execution error: {}", msg))
            }
            TspError::AlreadyRunning(msg) => {
                JsonRpcError::CustomError(-32002, format!("Task already running: {}", msg))
            }
        }
    }
}

/// Runtime state for a task
#[derive(Clone)]
struct TaskState {
    /// The task definition
    task: Task,

    /// Current status of the task
    status: TaskStatus,

    /// Last execution result
    result: Option<TaskResult>,

    /// Timestamp when the task was last started
    started_at: Option<Instant>,
}

/// Task Server that implements the Task Server Protocol
#[derive(Clone)]
pub struct TaskServer {
    /// Registry of available tasks
    tasks: Arc<RwLock<HashMap<String, TaskState>>>,

    /// Maps task names to their log channels
    log_channels: Arc<RwLock<HashMap<String, mpsc::Sender<LogEntry>>>>,

    /// Channel for broadcasting task status changes
    status_tx: mpsc::Sender<(String, TaskStatus)>,
}

impl TaskServer {
    /// Create a new TaskServer instance
    pub fn new() -> Self {
        let (status_tx, _) = mpsc::channel(100);

        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            log_channels: Arc::new(RwLock::new(HashMap::new())),
            status_tx,
        }
    }

    /// Register a task with the server
    pub async fn register_task(&self, task: Task) {
        let mut tasks = self.tasks.write().await;
        let task_name = task.name.clone();

        tasks.insert(
            task_name.clone(),
            TaskState {
                task,
                status: TaskStatus::Pending,
                result: None,
                started_at: None,
            },
        );

        // Setup logging channel for this task
        let (log_tx, _log_rx) = mpsc::channel(100);

        // Store the log sender so tasks can send logs
        self.log_channels
            .write()
            .await
            .insert(task_name.clone(), log_tx);

        debug!("Registered task: {}", task_name);
    }

    /// Start the server and listen on the specified Unix socket
    pub async fn start<P: AsRef<Path>>(&self, socket_path: P) -> Result<(), JsonRpcError> {
        let socket_path = socket_path.as_ref();

        // Remove the socket file if it exists
        if socket_path.exists() {
            if let Err(e) = fs::remove_file(socket_path).await {
                return Err(JsonRpcError::CustomError(
                    -32000,
                    format!("Failed to remove existing socket file: {}", e),
                ));
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent).await {
                    return Err(JsonRpcError::CustomError(
                        -32000,
                        format!("Failed to create parent directory: {}", e),
                    ));
                }
            }
        }

        // Create the socket path string
        let socket_path_str = format!("unix://{}", socket_path.to_string_lossy());

        // Start the server
        let server_builder = ServerBuilder::new(socket_path_str).map_err(|e| {
            JsonRpcError::CustomError(-32000, format!("Failed to create server builder: {}", e))
        })?;

        let server = server_builder
            .service(Arc::new(self.clone()))
            .build()
            .await
            .map_err(|e| {
                JsonRpcError::CustomError(-32000, format!("Failed to build server: {}", e))
            })?;

        // Start the notification handler
        self.handle_notifications();

        info!("TSP server listening on {}", socket_path.display());

        // Start the server
        server
            .start_block()
            .await
            .map_err(|e| JsonRpcError::CustomError(-32000, format!("Server error: {}", e)))
    }

    /// Handle internal notifications and broadcast them to clients
    fn handle_notifications(&self) {
        let server_clone = self.clone();

        // Create a new channel just for this notification handler
        let (internal_tx, mut internal_rx) = mpsc::channel::<(String, TaskStatus)>(100);

        // Keep a reference to the internal sender
        let internal_tx_clone = internal_tx.clone();

        // Forward messages from the main status channel to our internal channel
        tokio::spawn(async move {
            tokio::spawn(async move {
                while let Some((task_name, status)) = internal_rx.recv().await {
                    debug!("Task status change: {} -> {:?}", task_name, status);

                    // Update task status in our registry
                    if let Some(task_state) = server_clone.tasks.write().await.get_mut(&task_name) {
                        task_state.status = status.clone();

                        // If task just completed, update result
                        if matches!(
                            status,
                            TaskStatus::Success
                                | TaskStatus::Failed { .. }
                                | TaskStatus::Skipped { .. }
                        ) {
                            if let Some(started_at) = task_state.started_at {
                                let elapsed = started_at.elapsed();
                                let execution_time_ms = elapsed.as_millis() as u64;

                                // Create result
                                let result = TaskResult {
                                    name: task_name.clone(),
                                    status: status.clone(),
                                    output: json!({}), // Default empty output
                                    execution_time_ms,
                                };

                                task_state.result = Some(result);
                            }
                        }
                    }

                    // TODO: Broadcast notification to clients when we have notification support
                }
            });
        });

        // Store the sender for later use
        let _ = std::mem::replace(&mut self.status_tx.clone(), internal_tx_clone);
    }

    /// Implementation of the tsp_listTasks method
    async fn list_tasks(&self, _params: Value) -> RPCResult<Value> {
        let tasks = self.tasks.read().await;
        let task_list: Vec<Task> = tasks.values().map(|state| state.task.clone()).collect();

        Ok(serde_json::to_value(task_list).unwrap_or(json!([])))
    }

    /// Implementation of the tsp_getTask method
    async fn get_task(&self, params: Value) -> RPCResult<Value> {
        let task_name = params
            .as_str()
            .ok_or_else(|| TspError::InvalidParameters("Task name must be a string".to_string()))?;

        let tasks = self.tasks.read().await;
        let task_state = tasks
            .get(task_name)
            .ok_or_else(|| TspError::TaskNotFound(task_name.to_string()))?;

        Ok(serde_json::to_value(task_state.task.clone()).unwrap())
    }

    /// Implementation of the tsp_executeTask method
    async fn execute_task(&self, params: Value) -> RPCResult<Value> {
        let request: ExecuteTaskRequest = serde_json::from_value(params)
            .map_err(|e| TspError::InvalidParameters(format!("Invalid request: {}", e)))?;

        let task_name = request.name.clone();

        // Check if task exists
        let mut tasks = self.tasks.write().await;
        let task_state = tasks
            .get_mut(&task_name)
            .ok_or_else(|| TspError::TaskNotFound(task_name.clone()))?;

        // Check if task is already running
        if matches!(task_state.status, TaskStatus::Running) && !request.force {
            return Err(TspError::AlreadyRunning(task_name).into());
        }

        // Update task status to Running
        task_state.status = TaskStatus::Running;
        task_state.started_at = Some(Instant::now());

        // Clone data needed for task execution
        let task_name_clone = task_name.clone();
        let _input = request.input.clone();
        let _force = request.force;
        let status_tx = self.status_tx.clone();

        // Simulate task execution in a separate task
        tokio::spawn(async move {
            // Simulate some work
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Set task as completed
            let status = TaskStatus::Success;
            let _ = status_tx.send((task_name_clone, status)).await;
        });

        // Return preliminary result
        let result = TaskResult {
            name: task_name,
            status: TaskStatus::Running,
            output: json!({}),
            execution_time_ms: 0,
        };

        Ok(serde_json::to_value(result).unwrap())
    }

    /// Implementation of the tsp_getTaskStatus method
    async fn get_task_status(&self, params: Value) -> RPCResult<Value> {
        let task_name = params
            .as_str()
            .ok_or_else(|| TspError::InvalidParameters("Task name must be a string".to_string()))?;

        let tasks = self.tasks.read().await;
        let task_state = tasks
            .get(task_name)
            .ok_or_else(|| TspError::TaskNotFound(task_name.to_string()))?;

        Ok(json!({
            "status": task_state.status
        }))
    }
}

impl RPCService for TaskServer {
    fn get_method(&self, name: &str) -> Option<RPCMethod> {
        match name {
            methods::LIST_TASKS => Some(Box::new(move |params: Value| {
                Box::pin(self.list_tasks(params))
            })),
            methods::GET_TASK => Some(Box::new(move |params: Value| {
                Box::pin(self.get_task(params))
            })),
            methods::EXECUTE_TASK => Some(Box::new(move |params: Value| {
                Box::pin(self.execute_task(params))
            })),
            methods::GET_TASK_STATUS => Some(Box::new(move |params: Value| {
                Box::pin(self.get_task_status(params))
            })),
            _ => None,
        }
    }

    fn name(&self) -> String {
        "TaskServer".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    #[test]
    fn test_server_setup() {
        let rt = Runtime::new().unwrap();

        rt.block_on(async {
            let server = TaskServer::new();

            // Register a test task
            let task = Task {
                name: "test_task".to_string(),
                description: "A test task".to_string(),
                after: vec![],
                before: vec![],
                input: json!({}),
            };

            server.register_task(task).await;

            // Create temporary socket path
            let temp_dir = tempdir().unwrap();
            let socket_path = temp_dir.path().join("test_socket");

            // Start server in a separate task (it will block)
            let server_clone = server.clone();
            tokio::spawn(async move {
                let _ = server_clone.start(socket_path).await;
            });

            // Give server time to start
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Check that task is registered
            let tasks = server.tasks.read().await;
            assert!(tasks.contains_key("test_task"));
        });
    }
}
