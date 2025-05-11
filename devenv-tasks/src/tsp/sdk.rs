//! Task Server Protocol SDK
//!
//! This module provides a simple SDK for creating TSP-compatible task providers.
//! External crates can use this SDK to create CLIs that can register and execute
//! custom tasks with the devenv task system.
//!
/// Example usage of the TaskServerProtocol
///
/// ```rust,no_run
/// use devenv_tasks::tsp::protocol::Task;
/// use devenv_tasks::tsp::sdk::{TaskServerProtocol, TaskExecFuture};
/// use serde_json::json;
/// use std::pin::Pin;
///
/// #[tokio::main]
/// async fn main() -> eyre::Result<()> {
///     // Define tasks with their implementations
///     let tasks: Vec<(Task, TaskExecFuture)> = vec![
///         (
///             Task {
///                 name: "myapp:task1".to_string(),
///                 description: "My custom task".to_string(),
///                 after: vec![],
///                 before: vec![],
///                 input: json!({}),
///             },
///             Box::pin(async {
///                 println!("Executing custom task");
///                 // Your task implementation goes here
///                 Ok(())
///             }) as TaskExecFuture,
///         ),
///     ];
///
///     // Run the task provider (this will parse CLI args automatically)
///     TaskServerProtocol::run(tasks).await
/// }
/// ```
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::RwLock;
use tracing::info;

use super::protocol::Task;
use super::server::TaskServer;

/// Command line arguments for a TSP task provider CLI
#[derive(Parser, Debug, Clone)]
pub struct Args {
    /// Path to the Unix socket for connecting to the TSP server
    #[clap(long, env = "TSP_SOCKET", default_value = "/tmp/devenv-tsp.sock")]
    pub socket: PathBuf,

    /// Run in server mode (listening for connections instead of connecting)
    #[clap(long)]
    pub server: bool,

    /// Enable debug logging
    #[clap(long)]
    pub debug: bool,
}

/// Type alias for a task execution future
pub type TaskExecFuture = Pin<Box<dyn Future<Output = eyre::Result<()>> + Send>>;

/// The TaskServerProtocol provides a simple interface for registering and executing tasks
pub struct TaskServerProtocol {
    /// The TSP server instance
    server: TaskServer,

    /// Map of task names to their executors
    executors: Arc<RwLock<std::collections::HashMap<String, TaskExecFuture>>>,
}

impl TaskServerProtocol {
    /// Create a new TaskServerProtocol
    pub fn new() -> Self {
        let server = TaskServer::new();

        Self {
            server,
            executors: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Register a task with the server
    pub async fn register_task(&self, task: Task, executor: TaskExecFuture) -> eyre::Result<()> {
        // Register the task executor in our map
        self.executors
            .write()
            .await
            .insert(task.name.clone(), executor);

        // Register the task with the server
        self.server.register_task(task).await;

        Ok(())
    }

    /// Start the server and listen for connections
    pub async fn start_server(&self, socket_path: &PathBuf) -> eyre::Result<()> {
        // Start the server on the given Unix socket
        self.server
            .start(socket_path)
            .await
            .map_err(|e| eyre::eyre!("Failed to listen on Unix socket: {}", e))?;

        info!("TSP server listening on: {}", socket_path.display());

        // The server.start method is blocking, so we should never get here
        Ok(())
    }

    /// Run a task provider with the given tasks and their executor futures
    pub async fn run(tasks: Vec<(Task, TaskExecFuture)>) -> eyre::Result<()> {
        // Parse command line arguments
        let args = Args::parse();

        // Create a new protocol instance
        let protocol = Self::new();

        // Register all provided tasks with their executors
        for (task, executor) in tasks {
            protocol.register_task(task, executor).await?;
        }

        // If running in server mode, start the server
        if args.server {
            protocol.start_server(&args.socket).await?;
        } else {
            // Otherwise, connect to an existing server
            info!("Connecting to TSP server at: {}", args.socket.display());
            // Code to connect to existing server would go here
            // This is not fully implemented in this example
        }

        Ok(())
    }
}

impl Default for TaskServerProtocol {
    fn default() -> Self {
        Self::new()
    }
}
