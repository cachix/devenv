//! Resource manager for eval cache integration.
//!
//! Manages multiple `ReplayableResource` implementations and coordinates
//! snapshotting after evaluation and replay on cache hit.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use devenv_core::ports::{PortAllocator, PortSpec};
use devenv_core::resource::{ReplayError, ReplayableResource};

/// A serialized resource spec stored in the cache.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceSpec {
    /// The type identifier (e.g., "ports").
    pub type_id: String,
    /// The JSON-serialized spec data.
    pub data: serde_json::Value,
}

/// Manages multiple resource types for cache integration.
///
/// Coordinates snapshotting after evaluation and replay on cache hit.
/// Currently supports port allocations, but designed to be extensible
/// to other resource types (temp dirs, unique IDs, etc.).
pub struct ResourceManager {
    port_allocator: Arc<PortAllocator>,
    // Future: temp_allocator: Arc<TempAllocator>,
}

impl ResourceManager {
    /// Create a new resource manager.
    pub fn new(port_allocator: Arc<PortAllocator>) -> Self {
        Self { port_allocator }
    }

    /// Get a reference to the port allocator.
    pub fn port_allocator(&self) -> &Arc<PortAllocator> {
        &self.port_allocator
    }

    /// Snapshot all resources after evaluation.
    ///
    /// Returns a list of resource specs to store in the cache.
    /// Empty specs (no allocations) are omitted.
    pub fn snapshot_all(&self) -> Vec<ResourceSpec> {
        let mut specs = Vec::new();

        // Snapshot ports
        let port_spec = self.port_allocator.snapshot();
        if !port_spec.allocations.is_empty() {
            specs.push(ResourceSpec {
                type_id: PortAllocator::TYPE_ID.to_string(),
                data: serde_json::to_value(&port_spec)
                    .expect("PortSpec serialization should not fail"),
            });
        }

        // Future: snapshot other resources
        // let temp_spec = self.temp_allocator.snapshot();
        // if !temp_spec.is_empty() { ... }

        specs
    }

    /// Replay all resources from cached specs.
    ///
    /// Called on cache hit to re-acquire resources that were allocated
    /// during the original evaluation.
    ///
    /// Returns Ok(()) if all resources were successfully re-acquired.
    /// Returns Err if any resource failed to replay (cache should be invalidated).
    pub fn replay_all(&self, specs: &[ResourceSpec]) -> Result<(), ReplayError> {
        for spec in specs {
            match spec.type_id.as_str() {
                "ports" => {
                    let port_spec: PortSpec = serde_json::from_value(spec.data.clone())
                        .map_err(|e| ReplayError::Serialization(e.to_string()))?;
                    self.port_allocator.replay(&port_spec)?;
                }
                other => {
                    return Err(ReplayError::Unavailable(format!(
                        "Unknown resource type: {}",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    /// Clear all resource allocations.
    ///
    /// Called after a replay failure to reset state before re-evaluation.
    pub fn clear_all(&self) {
        self.port_allocator.clear();
        // Future: self.temp_allocator.clear();
    }

    /// Check if any resources have been allocated.
    pub fn has_allocations(&self) -> bool {
        !self.port_allocator.snapshot().allocations.is_empty()
        // Future: || !self.temp_allocator.snapshot().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_empty() {
        let allocator = Arc::new(PortAllocator::new());
        let manager = ResourceManager::new(allocator);

        let specs = manager.snapshot_all();
        assert!(specs.is_empty());
    }

    #[test]
    fn test_snapshot_with_ports() {
        let allocator = Arc::new(PortAllocator::new());
        allocator.set_enabled(true);
        allocator.allocate("server", "http", 50000).unwrap();

        let manager = ResourceManager::new(allocator);
        let specs = manager.snapshot_all();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].type_id, "ports");
    }

    #[test]
    fn test_replay_success() {
        let allocator = Arc::new(PortAllocator::new());
        allocator.set_enabled(true);
        let port = allocator.allocate("server", "http", 50100).unwrap();

        let manager = ResourceManager::new(allocator.clone());
        let specs = manager.snapshot_all();

        // Clear and release
        drop(allocator.take_reservations());
        allocator.clear();

        // Replay
        manager.replay_all(&specs).unwrap();

        // Verify port was re-acquired
        let new_specs = manager.snapshot_all();
        assert_eq!(new_specs.len(), 1);

        let port_spec: PortSpec = serde_json::from_value(new_specs[0].data.clone()).unwrap();
        assert_eq!(port_spec.allocations[0].allocated_port, port);
    }

    #[test]
    fn test_replay_unknown_type() {
        let allocator = Arc::new(PortAllocator::new());
        let manager = ResourceManager::new(allocator);

        let specs = vec![ResourceSpec {
            type_id: "unknown".to_string(),
            data: serde_json::json!({}),
        }];

        let err = manager.replay_all(&specs).unwrap_err();
        assert!(matches!(err, ReplayError::Unavailable(_)));
    }

    #[test]
    fn test_clear_all() {
        let allocator = Arc::new(PortAllocator::new());
        allocator.set_enabled(true);
        allocator.allocate("server", "http", 50200).unwrap();

        let manager = ResourceManager::new(allocator);
        assert!(manager.has_allocations());

        manager.clear_all();
        assert!(!manager.has_allocations());
    }
}
