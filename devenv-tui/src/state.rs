use crate::{
    LogMessage, NixActivityState, NixBuildInfo, NixDerivationInfo, NixDownloadInfo, NixQueryInfo,
    Operation, OperationId, OperationResult, TuiEvent,
};
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
    nix_derivations: HashMap<u64, NixDerivationInfo>,
    nix_downloads: HashMap<u64, NixDownloadInfo>,
    nix_queries: HashMap<u64, NixQueryInfo>,
    root_operations: Vec<OperationId>,
}

impl TuiState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TuiStateInner {
                operations: HashMap::new(),
                message_log: VecDeque::new(),
                nix_builds: HashMap::new(),
                nix_derivations: HashMap::new(),
                nix_downloads: HashMap::new(),
                nix_queries: HashMap::new(),
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
                machine: _,
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

            TuiEvent::NixDerivationStart {
                operation_id,
                activity_id,
                derivation_path,
                derivation_name,
                machine,
            } => {
                let derivation_info = NixDerivationInfo {
                    operation_id,
                    activity_id,
                    derivation_path,
                    derivation_name,
                    machine,
                    current_phase: None,
                    start_time: std::time::Instant::now(),
                    state: NixActivityState::Active,
                };
                inner.nix_derivations.insert(activity_id, derivation_info);
            }

            TuiEvent::NixPhaseProgress {
                operation_id: _,
                activity_id,
                phase,
            } => {
                if let Some(derivation_info) = inner.nix_derivations.get_mut(&activity_id) {
                    derivation_info.current_phase = Some(phase);
                }
            }

            TuiEvent::NixDerivationEnd {
                operation_id: _,
                activity_id,
                success,
            } => {
                if let Some(derivation_info) = inner.nix_derivations.get_mut(&activity_id) {
                    let duration = derivation_info.start_time.elapsed();
                    derivation_info.state = NixActivityState::Completed { success, duration };
                }
            }

            TuiEvent::NixDownloadStart {
                operation_id,
                activity_id,
                store_path,
                package_name,
                substituter,
            } => {
                let download_info = NixDownloadInfo {
                    operation_id,
                    activity_id,
                    store_path,
                    package_name,
                    substituter,
                    bytes_downloaded: 0,
                    total_bytes: None,
                    start_time: std::time::Instant::now(),
                    state: NixActivityState::Active,
                };
                inner.nix_downloads.insert(activity_id, download_info);
            }

            TuiEvent::NixDownloadProgress {
                operation_id: _,
                activity_id,
                bytes_downloaded,
                total_bytes,
            } => {
                if let Some(download_info) = inner.nix_downloads.get_mut(&activity_id) {
                    download_info.bytes_downloaded = bytes_downloaded;
                    if total_bytes.is_some() {
                        download_info.total_bytes = total_bytes;
                    }
                }
            }

            TuiEvent::NixDownloadEnd {
                operation_id: _,
                activity_id,
                success,
            } => {
                if let Some(download_info) = inner.nix_downloads.get_mut(&activity_id) {
                    let duration = download_info.start_time.elapsed();
                    download_info.state = NixActivityState::Completed { success, duration };
                }
            }

            TuiEvent::NixQueryStart {
                operation_id,
                activity_id,
                store_path,
                package_name,
                substituter,
            } => {
                let query_info = NixQueryInfo {
                    operation_id,
                    activity_id,
                    store_path,
                    package_name,
                    substituter,
                    start_time: std::time::Instant::now(),
                    state: NixActivityState::Active,
                };
                inner.nix_queries.insert(activity_id, query_info);
            }

            TuiEvent::NixQueryEnd {
                operation_id: _,
                activity_id,
                success,
            } => {
                if let Some(query_info) = inner.nix_queries.get_mut(&activity_id) {
                    let duration = query_info.start_time.elapsed();
                    query_info.state = NixActivityState::Completed { success, duration };
                }
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

    /// Get all active Nix derivations for an operation
    pub fn get_nix_derivations_for_operation(
        &self,
        operation_id: &OperationId,
    ) -> Vec<NixDerivationInfo> {
        let inner = self.inner.lock().unwrap();
        inner
            .nix_derivations
            .values()
            .filter(|info| {
                &info.operation_id == operation_id && info.state == NixActivityState::Active
            })
            .cloned()
            .collect()
    }

    /// Get all active Nix downloads for an operation
    pub fn get_nix_downloads_for_operation(
        &self,
        operation_id: &OperationId,
    ) -> Vec<NixDownloadInfo> {
        let inner = self.inner.lock().unwrap();
        inner
            .nix_downloads
            .values()
            .filter(|info| {
                &info.operation_id == operation_id && info.state == NixActivityState::Active
            })
            .cloned()
            .collect()
    }

    /// Get all active Nix queries for an operation
    pub fn get_nix_queries_for_operation(&self, operation_id: &OperationId) -> Vec<NixQueryInfo> {
        let inner = self.inner.lock().unwrap();
        inner
            .nix_queries
            .values()
            .filter(|info| {
                &info.operation_id == operation_id && info.state == NixActivityState::Active
            })
            .cloned()
            .collect()
    }

    /// Get all Nix activities (derivations, downloads, queries) for an operation
    pub fn get_all_nix_activities_for_operation(
        &self,
        operation_id: &OperationId,
    ) -> (
        Vec<NixDerivationInfo>,
        Vec<NixDownloadInfo>,
        Vec<NixQueryInfo>,
    ) {
        let inner = self.inner.lock().unwrap();

        let derivations = inner
            .nix_derivations
            .values()
            .filter(|info| &info.operation_id == operation_id)
            .cloned()
            .collect();

        let downloads = inner
            .nix_downloads
            .values()
            .filter(|info| &info.operation_id == operation_id)
            .cloned()
            .collect();

        let queries = inner
            .nix_queries
            .values()
            .filter(|info| &info.operation_id == operation_id)
            .cloned()
            .collect();

        (derivations, downloads, queries)
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
