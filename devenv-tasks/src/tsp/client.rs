//! Task Server Protocol client implementation
//!
//! This module provides a client implementation for the Task Server Protocol,
//! which allows applications to communicate with TSP servers via Unix sockets.

use std::path::Path;
use std::sync::Arc;

use karyon_jsonrpc::{
    client::Client as JsonRpcClient, client::ClientBuilder, codec::JsonCodec,
    error::RPCError as JsonRpcError,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::debug;

use super::protocol::{methods, ExecuteTaskRequest, LogEntry, Task, TaskResult, TaskStatus};

/// A client for interacting with a Task Server Protocol server over Unix sockets
pub struct TaskClient {
    /// The JSON-RPC client
    client: Arc<JsonRpcClient<JsonCodec>>,

    /// Receiver for log entries
    log_rx: Option<mpsc::Receiver<LogEntry>>,

    /// Receiver for status updates
    status_rx: Option<mpsc::Receiver<(String, TaskStatus)>>,
}

impl TaskClient {
    /// Create a new TaskClient that connects to a Unix socket at the given path
    pub async fn connect<P: AsRef<Path>>(socket_path: P) -> Result<Self, JsonRpcError> {
        // Convert Path to &str for endpoint
        let socket_path_str = socket_path.as_ref().to_string_lossy().to_string();

        // Use ClientBuilder to build a Unix socket client
        let builder = ClientBuilder::new(format!("unix://{}", socket_path_str)).map_err(|e| {
            JsonRpcError::CustomError(-32001, format!("Failed to connect to Unix socket: {}", e))
        })?;

        let client = builder.build().await.map_err(|e| {
            JsonRpcError::CustomError(-32001, format!("Failed to connect to Unix socket: {}", e))
        })?;

        Ok(Self {
            client,
            log_rx: None,
            status_rx: None,
        })
    }

    /// Initialize the client and set up notification handlers
    ///
    /// This method will attempt to set up notification channels,
    /// but notification handling is a bit limited due to client constraints.
    pub async fn init(&mut self) -> Result<(), JsonRpcError> {
        // Set up channels for notifications
        let (_log_tx, log_rx) = mpsc::channel(100);
        let (_status_tx, status_rx) = mpsc::channel(100);

        self.log_rx = Some(log_rx);
        self.status_rx = Some(status_rx);

        // Normally we would set up notification handlers here,
        // but the client is already behind an Arc, so we can't easily move it to another task.
        // For now, we'll just return without setting up notification handling.
        debug!("Client initialized, but notification handling is not implemented");

        Ok(())
    }

    /// Get the log receiver
    pub fn log_receiver(&mut self) -> Option<mpsc::Receiver<LogEntry>> {
        self.log_rx.take()
    }

    /// Get the status update receiver
    pub fn status_receiver(&mut self) -> Option<mpsc::Receiver<(String, TaskStatus)>> {
        self.status_rx.take()
    }

    /// Helper method to send a request and parse the response
    async fn send_request<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, JsonRpcError> {
        let params_value = serde_json::to_value(params).map_err(|e| {
            JsonRpcError::InvalidParams(format!("Failed to serialize parameters: {}", e))
        })?;

        // Send the request and handle errors
        let response =
            self.client.call(method, params_value).await.map_err(|e| {
                JsonRpcError::CustomError(-32001, format!("RPC call failed: {}", e))
            })?;

        serde_json::from_value(response)
            .map_err(|e| JsonRpcError::ParseError(format!("Failed to deserialize response: {}", e)))
    }

    /// List all available tasks
    pub async fn list_tasks(&self) -> Result<Vec<Task>, JsonRpcError> {
        self.send_request::<(), Vec<Task>>(methods::LIST_TASKS, ())
            .await
    }

    /// Get information about a specific task
    pub async fn get_task(&self, name: &str) -> Result<Task, JsonRpcError> {
        self.send_request::<String, Task>(methods::GET_TASK, name.to_string())
            .await
    }

    /// Execute a task
    pub async fn execute_task(
        &self,
        name: &str,
        input: Value,
        force: bool,
    ) -> Result<TaskResult, JsonRpcError> {
        let request = ExecuteTaskRequest {
            name: name.to_string(),
            input,
            force,
        };

        self.send_request::<ExecuteTaskRequest, TaskResult>(methods::EXECUTE_TASK, request)
            .await
    }

    /// Get the current status of a task
    pub async fn get_task_status(&self, name: &str) -> Result<TaskStatus, JsonRpcError> {
        #[derive(serde::Deserialize)]
        struct StatusResponse {
            status: TaskStatus,
        }

        let response: StatusResponse = self
            .send_request::<String, StatusResponse>(methods::GET_TASK_STATUS, name.to_string())
            .await?;

        Ok(response.status)
    }
}
