use crate::{LogMessage, NixBuildInfo, Operation, OperationId, OperationResult, TuiEvent};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Maximum number of log messages to keep in memory
const MAX_LOG_MESSAGES: usize = 1000;

/// Central state management for the TUI
pub struct TuiState {
    inner: Arc<Mutex<TuiStateInner>>,
}

struct TuiStateInner {
    operations: HashMap<OperationId, Operation>,
    message_log: VecDeque<LogMessage>,
    nix_builds: HashMap<OperationId, NixBuildInfo>,
    root_operations: Vec<OperationId>,
}

impl TuiState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TuiStateInner {
                operations: HashMap::new(),
                message_log: VecDeque::new(),
                nix_builds: HashMap::new(),
                root_operations: Vec::new(),
            })),
        }
    }

    /// Process a TUI event and update state
    pub fn handle_event(&self, event: TuiEvent) {
        let mut inner = self.inner.lock().unwrap();

        match event {
            TuiEvent::OperationStart {
                id,
                message,
                parent,
                data,
            } => {
                let operation = Operation::new(id.clone(), message, parent.clone(), data);

                // Add to parent's children if parent exists
                if let Some(parent_id) = &parent {
                    if let Some(parent_op) = inner.operations.get_mut(parent_id) {
                        parent_op.children.push(id.clone());
                    }
                } else {
                    // Root operation
                    inner.root_operations.push(id.clone());
                }

                inner.operations.insert(id, operation);
            }

            TuiEvent::OperationEnd { id, result } => {
                if let Some(operation) = inner.operations.get_mut(&id) {
                    let success = matches!(result, OperationResult::Success);
                    operation.complete(success);
                }
            }

            TuiEvent::LogMessage {
                level,
                message,
                source,
                data,
            } => {
                let log_msg = LogMessage::new(level, message, source, data);
                inner.message_log.push_back(log_msg);

                // Keep log size bounded
                if inner.message_log.len() > MAX_LOG_MESSAGES {
                    inner.message_log.pop_front();
                }
            }

            TuiEvent::NixBuildStart {
                operation_id,
                derivation,
            } => {
                let build_info = NixBuildInfo {
                    operation_id: operation_id.clone(),
                    derivation,
                    current_phase: None,
                    start_time: std::time::Instant::now(),
                };
                inner.nix_builds.insert(operation_id, build_info);
            }

            TuiEvent::NixBuildProgress {
                operation_id,
                phase,
            } => {
                if let Some(build_info) = inner.nix_builds.get_mut(&operation_id) {
                    build_info.current_phase = Some(phase);
                }
            }

            TuiEvent::NixBuildEnd {
                operation_id,
                success: _,
            } => {
                inner.nix_builds.remove(&operation_id);
            }

            TuiEvent::Shutdown => {
                // No state changes needed for shutdown
            }
        }
    }

    /// Get all active operations (non-completed operations)
    pub fn get_active_operations(&self) -> Vec<Operation> {
        let inner = self.inner.lock().unwrap();
        inner
            .operations
            .values()
            .filter(|op| matches!(op.state, crate::OperationState::Active))
            .cloned()
            .collect()
    }

    /// Get all root operations (operations without parents)
    pub fn get_root_operations(&self) -> Vec<Operation> {
        let inner = self.inner.lock().unwrap();
        inner
            .root_operations
            .iter()
            .filter_map(|id| inner.operations.get(id))
            .cloned()
            .collect()
    }

    /// Get recent log messages
    pub fn get_recent_log_messages(&self, count: usize) -> Vec<LogMessage> {
        let inner = self.inner.lock().unwrap();
        inner
            .message_log
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    /// Get operation by ID
    pub fn get_operation(&self, id: &OperationId) -> Option<Operation> {
        let inner = self.inner.lock().unwrap();
        inner.operations.get(id).cloned()
    }

    /// Get children of an operation
    pub fn get_children(&self, parent_id: &OperationId) -> Vec<Operation> {
        let inner = self.inner.lock().unwrap();
        if let Some(parent) = inner.operations.get(parent_id) {
            parent
                .children
                .iter()
                .filter_map(|child_id| inner.operations.get(child_id))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get Nix build info for an operation
    pub fn get_nix_build(&self, operation_id: &OperationId) -> Option<NixBuildInfo> {
        let inner = self.inner.lock().unwrap();
        inner.nix_builds.get(operation_id).cloned()
    }

    /// Clean up completed operations that are older than a certain threshold
    pub fn cleanup_old_operations(&self, max_age: std::time::Duration) {
        let mut inner = self.inner.lock().unwrap();
        let now = std::time::Instant::now();

        let mut to_remove = Vec::new();
        for (id, operation) in &inner.operations {
            if let crate::OperationState::Complete {
                duration: _,
                success: _,
            } = operation.state
            {
                if now.duration_since(operation.start_time) > max_age {
                    to_remove.push(id.clone());
                }
            }
        }

        for id in to_remove {
            inner.operations.remove(&id);
            inner.root_operations.retain(|op_id| *op_id != id);
            // Note: We don't remove from children lists here for simplicity,
            // but in a production implementation you might want to clean those up too
        }
    }
}

impl Default for TuiState {
    fn default() -> Self {
        Self::new()
    }
}
