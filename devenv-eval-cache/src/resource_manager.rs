//! Type-erased replayable resources for eval-cache integration.
//!
//! Evaluation extensions register the resources they acquire while evaluating.
//! The registry deliberately knows neither their concrete types nor their
//! serialized formats; resource implementations provide both through
//! [`ReplayableResource`].

use std::{collections::BTreeMap, sync::Arc};

use devenv_core::resource::{ReplayError, ReplayableResource};
use serde::{Deserialize, Serialize};

/// A serialized replayable resource stored alongside a cached evaluation.
///
/// `type_id` and `data` intentionally retain the cache's existing on-disk
/// representation so adding the registry does not require a cache migration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvalResourceSpec {
    /// The resource's unique type identifier.
    pub type_id: String,
    /// JSON-serialized resource state.
    pub data: serde_json::Value,
}

trait ErasedEvalResource: Send + Sync {
    fn is_empty(&self) -> bool;
    fn snapshot(&self) -> Result<serde_json::Value, ReplayError>;
    fn replay(&self, data: &serde_json::Value) -> Result<(), ReplayError>;
    fn clear(&self);
}

struct EvalResource<R>(Arc<R>);

impl<R> ErasedEvalResource for EvalResource<R>
where
    R: ReplayableResource + 'static,
{
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn snapshot(&self) -> Result<serde_json::Value, ReplayError> {
        serde_json::to_value(self.0.snapshot())
            .map_err(|error| ReplayError::Serialization(error.to_string()))
    }

    fn replay(&self, data: &serde_json::Value) -> Result<(), ReplayError> {
        let spec = serde_json::from_value(data.clone())
            .map_err(|error| ReplayError::Serialization(error.to_string()))?;
        self.0.replay(&spec)
    }

    fn clear(&self) {
        self.0.clear();
    }
}

/// Registry of resources acquired during Nix evaluation.
///
/// Resource registration happens when the evaluator is composed. The registry
/// then snapshots every non-empty resource after evaluation and dispatches
/// cached specifications by stable type identifier on cache hits.
#[derive(Default)]
pub struct EvalResourceRegistry {
    resources: BTreeMap<&'static str, Box<dyn ErasedEvalResource>>,
}

impl EvalResourceRegistry {
    /// Create an empty resource registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a replayable resource.
    ///
    /// A resource type ID may only be registered once. Duplicate IDs are a
    /// composition error because a cached spec could otherwise be dispatched
    /// to the wrong resource implementation.
    pub fn register<R>(&mut self, resource: Arc<R>) -> &mut Self
    where
        R: ReplayableResource + 'static,
    {
        assert!(
            !self.resources.contains_key(R::TYPE_ID),
            "replayable resource type ID `{}` registered more than once",
            R::TYPE_ID
        );
        self.resources
            .insert(R::TYPE_ID, Box::new(EvalResource(resource)));
        self
    }

    /// Return whether this registry can replay `type_id`.
    pub fn handles(&self, type_id: &str) -> bool {
        self.resources.contains_key(type_id)
    }

    /// Snapshot all non-empty resources for storage in the eval cache.
    pub fn snapshot_all(&self) -> Result<Vec<EvalResourceSpec>, ReplayError> {
        self.resources
            .iter()
            .filter(|(_, resource)| !resource.is_empty())
            .map(|(type_id, resource)| {
                Ok(EvalResourceSpec {
                    type_id: (*type_id).to_owned(),
                    data: resource.snapshot()?,
                })
            })
            .collect()
    }

    /// Replay every resource stored in a cached evaluation.
    ///
    /// Any unknown resource or replay failure means the cache entry cannot be
    /// safely reused. Call [`Self::clear_all`] before re-evaluating after an
    /// error to release resources replayed earlier in the sequence.
    pub fn replay_all(&self, specs: &[EvalResourceSpec]) -> Result<(), ReplayError> {
        for spec in specs {
            let resource = self.resources.get(spec.type_id.as_str()).ok_or_else(|| {
                ReplayError::Unavailable(format!("Unknown resource type: {}", spec.type_id))
            })?;
            resource.replay(&spec.data)?;
        }
        Ok(())
    }

    /// Clear every registered resource.
    pub fn clear_all(&self) {
        for resource in self.resources.values() {
            resource.clear();
        }
    }

    /// Return whether any registered resource currently holds allocations.
    pub fn has_resources(&self) -> bool {
        self.resources.values().any(|resource| !resource.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use devenv_core::ports::{PortAllocator, PortSpec};
    use serde::{Deserialize, Serialize};

    #[derive(Default)]
    struct TestResource(Mutex<Vec<String>>);

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct TestSpec(Vec<String>);

    impl ReplayableResource for TestResource {
        type Spec = TestSpec;
        const TYPE_ID: &'static str = "test-resource";

        fn snapshot(&self) -> Self::Spec {
            TestSpec(self.0.lock().unwrap().clone())
        }

        fn is_empty(&self) -> bool {
            self.0.lock().unwrap().is_empty()
        }

        fn replay(&self, spec: &Self::Spec) -> Result<(), ReplayError> {
            *self.0.lock().unwrap() = spec.0.clone();
            Ok(())
        }

        fn clear(&self) {
            self.0.lock().unwrap().clear();
        }
    }

    #[test]
    fn snapshot_omits_empty_resources() {
        let resource = Arc::new(TestResource::default());
        let mut registry = EvalResourceRegistry::new();
        registry.register(resource);

        assert!(registry.snapshot_all().unwrap().is_empty());
    }

    #[test]
    fn snapshots_and_replays_registered_resource_without_type_dispatch() {
        let resource = Arc::new(TestResource(Mutex::new(vec!["one".to_owned()])));
        let mut registry = EvalResourceRegistry::new();
        registry.register(resource.clone());

        let specs = registry.snapshot_all().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].type_id, TestResource::TYPE_ID);
        assert!(registry.handles(TestResource::TYPE_ID));

        registry.clear_all();
        assert!(!registry.has_resources());
        registry.replay_all(&specs).unwrap();
        assert_eq!(*resource.0.lock().unwrap(), vec!["one"]);
    }

    #[test]
    fn replays_ports_through_the_generic_registry() {
        let allocator = Arc::new(PortAllocator::new());
        allocator.set_enabled(true);
        let port = allocator.allocate("server", "http", 50100).unwrap();

        let mut registry = EvalResourceRegistry::new();
        registry.register(allocator.clone());
        let specs = registry.snapshot_all().unwrap();

        drop(allocator.take_reservations());
        registry.clear_all();
        registry.replay_all(&specs).unwrap();

        let replayed: PortSpec =
            serde_json::from_value(registry.snapshot_all().unwrap()[0].data.clone()).unwrap();
        assert_eq!(replayed.allocations[0].allocated_port, port);
    }

    #[test]
    fn rejects_unknown_resource_type() {
        let registry = EvalResourceRegistry::new();
        let error = registry
            .replay_all(&[EvalResourceSpec {
                type_id: "unknown".to_owned(),
                data: serde_json::json!({}),
            }])
            .unwrap_err();

        assert!(matches!(error, ReplayError::Unavailable(_)));
    }
}
