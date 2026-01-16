//! Replayable resource abstractions for eval caching.
//!
//! During Nix evaluation, side effects like port allocation occur. These need to be:
//! 1. Captured (snapshotted) after evaluation completes
//! 2. Stored in the eval cache alongside the result
//! 3. Replayed on cache hit to restore the same state
//! 4. Used to invalidate the cache if replay fails
//!
//! This module provides the `ReplayableResource` trait that abstracts over
//! different resource types (ports, temp dirs, etc.) that need this behavior.

use serde::{Serialize, de::DeserializeOwned};

/// A resource allocator that can snapshot its state and replay from a spec.
///
/// The allocator maintains state during evaluation. After eval completes,
/// `snapshot()` captures what was allocated. On cache hit, `replay()`
/// restores that state from the cached spec.
///
/// # Example
///
/// ```ignore
/// // After evaluation completes:
/// let spec = port_allocator.snapshot();
/// cache.store(result, spec);
///
/// // On cache hit:
/// if let Err(e) = port_allocator.replay(&cached_spec) {
///     // Replay failed, cache is invalid
///     port_allocator.clear();
///     return None; // Trigger re-evaluation
/// }
/// ```
pub trait ReplayableResource: Send + Sync {
    /// The spec type that describes allocated resources.
    /// Must be serializable for cache storage.
    type Spec: Clone + Serialize + DeserializeOwned + Send + Sync;

    /// Unique type identifier for database storage.
    /// Example: "ports", "tempdirs", "unique_ids"
    const TYPE_ID: &'static str;

    /// Snapshot current allocations as a spec.
    /// Called after evaluation completes.
    fn snapshot(&self) -> Self::Spec;

    /// Replay allocations from a cached spec.
    /// Called on cache hit before returning cached result.
    /// Returns Ok(()) if all resources re-acquired, Err if any unavailable.
    fn replay(&self, spec: &Self::Spec) -> Result<(), ReplayError>;

    /// Clear all allocations (for fresh eval after replay failure).
    fn clear(&self);
}

/// Errors during resource replay.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    /// The resource could not be acquired (e.g., port in use).
    #[error("Resource unavailable: {0}")]
    Unavailable(String),

    /// The resource conflicts with existing allocations.
    #[error("Resource conflict: {0}")]
    Conflict(String),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A simple test resource for verifying the trait design.
    struct TestAllocator {
        values: Mutex<Vec<u32>>,
    }

    #[derive(Clone, Debug, Serialize, serde::Deserialize)]
    struct TestSpec {
        values: Vec<u32>,
    }

    impl ReplayableResource for TestAllocator {
        type Spec = TestSpec;
        const TYPE_ID: &'static str = "test";

        fn snapshot(&self) -> TestSpec {
            let values = self.values.lock().unwrap();
            TestSpec {
                values: values.clone(),
            }
        }

        fn replay(&self, spec: &TestSpec) -> Result<(), ReplayError> {
            let mut values = self.values.lock().unwrap();
            for &v in &spec.values {
                if v == 0 {
                    return Err(ReplayError::Unavailable("zero not allowed".to_string()));
                }
                values.push(v);
            }
            Ok(())
        }

        fn clear(&self) {
            let mut values = self.values.lock().unwrap();
            values.clear();
        }
    }

    #[test]
    fn test_snapshot_and_replay() {
        let allocator = TestAllocator {
            values: Mutex::new(vec![1, 2, 3]),
        };

        let spec = allocator.snapshot();
        assert_eq!(spec.values, vec![1, 2, 3]);

        allocator.clear();
        assert!(allocator.values.lock().unwrap().is_empty());

        allocator.replay(&spec).unwrap();
        assert_eq!(*allocator.values.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_replay_failure() {
        let allocator = TestAllocator {
            values: Mutex::new(vec![]),
        };

        let spec = TestSpec { values: vec![0] };
        let err = allocator.replay(&spec).unwrap_err();
        assert!(matches!(err, ReplayError::Unavailable(_)));
    }

    #[test]
    fn test_type_id() {
        assert_eq!(TestAllocator::TYPE_ID, "test");
    }
}
