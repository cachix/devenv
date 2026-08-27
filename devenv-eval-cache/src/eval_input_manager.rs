//! Eval-input management for eval-cache integration.
//!
//! Eval inputs are values observed by an eval, such as dotenv
//! files. They are deliberately distinct from replayable resources: a cache
//! hit validates inputs before it is used, rather than trying to acquire them.

use std::collections::BTreeMap;
use std::sync::Arc;

use devenv_core::resource::{EvalInput, ReplayError};
use serde::{Deserialize, Serialize};

/// A serialized eval-input snapshot stored alongside a cached eval.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvalInputSpec {
    /// The registered input type identifier.
    pub type_id: String,
    /// The JSON-serialized input state.
    pub data: serde_json::Value,
}

trait ErasedEvalInput: Send + Sync {
    fn is_empty(&self) -> bool;
    fn snapshot(&self) -> Result<serde_json::Value, ReplayError>;
    fn changed(&self, data: &serde_json::Value) -> Result<bool, ReplayError>;
    fn validate(&self, data: &serde_json::Value) -> Result<(), ReplayError>;
    fn restore(&self, data: &serde_json::Value) -> Result<(), ReplayError>;
    fn clear(&self);
}

struct RegisteredEvalInput<T> {
    input: Arc<T>,
}

impl<T> ErasedEvalInput for RegisteredEvalInput<T>
where
    T: EvalInput + 'static,
{
    fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    fn snapshot(&self) -> Result<serde_json::Value, ReplayError> {
        serde_json::to_value(self.input.snapshot())
            .map_err(|error| ReplayError::Serialization(error.to_string()))
    }

    fn validate(&self, data: &serde_json::Value) -> Result<(), ReplayError> {
        let spec = serde_json::from_value(data.clone())
            .map_err(|error| ReplayError::Serialization(error.to_string()))?;
        self.input.validate(&spec)
    }

    fn changed(&self, data: &serde_json::Value) -> Result<bool, ReplayError> {
        let spec = serde_json::from_value(data.clone())
            .map_err(|error| ReplayError::Serialization(error.to_string()))?;
        self.input.changed(&spec)
    }

    fn restore(&self, data: &serde_json::Value) -> Result<(), ReplayError> {
        let spec = serde_json::from_value(data.clone())
            .map_err(|error| ReplayError::Serialization(error.to_string()))?;
        self.input.restore(&spec)
    }

    fn clear(&self) {
        self.input.clear();
    }
}

/// Type-erased registry of inputs that affect an evaluation.
///
/// Register concrete [`EvalInput`] implementations while constructing
/// the evaluator. The manager owns no input state; it only serializes and
/// dispatches to the shared input trackers.
#[derive(Default)]
pub struct EvalInputManager {
    inputs: BTreeMap<&'static str, Box<dyn ErasedEvalInput>>,
}

impl EvalInputManager {
    /// Create an empty input registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an input tracker.
    ///
    /// Each input type can be registered once. Type identifiers are persisted
    /// in the cache and must therefore remain stable and unique.
    pub fn register<T>(&mut self, input: Arc<T>) -> &mut Self
    where
        T: EvalInput + 'static,
    {
        assert!(
            !self.inputs.contains_key(T::TYPE_ID),
            "eval input type '{}' registered more than once",
            T::TYPE_ID
        );
        self.inputs
            .insert(T::TYPE_ID, Box::new(RegisteredEvalInput { input }));
        self
    }

    /// Snapshot every non-empty input for cache storage.
    pub fn snapshot_all(&self) -> Result<Vec<EvalInputSpec>, ReplayError> {
        self.inputs
            .iter()
            .filter(|(_, input)| !input.is_empty())
            .map(|(type_id, input)| {
                Ok(EvalInputSpec {
                    type_id: (*type_id).to_owned(),
                    data: input.snapshot()?,
                })
            })
            .collect()
    }

    /// Shorthand for [`Self::snapshot_all`], useful when retaining a snapshot
    /// across task execution.
    pub fn snapshot(&self) -> Result<Vec<EvalInputSpec>, ReplayError> {
        self.snapshot_all()
    }

    /// Validate all cached input snapshots without changing tracker state.
    pub fn validate_all(&self, specs: &[EvalInputSpec]) -> Result<(), ReplayError> {
        for spec in specs {
            self.input(&spec.type_id)?.validate(&spec.data)?;
        }
        Ok(())
    }

    /// Validate and restore cached input snapshots into their live trackers.
    ///
    /// Existing recorded inputs are cleared first, so a cache entry with no
    /// spec does not leave dependencies from an earlier evaluation behind.
    pub fn restore_all(&self, specs: &[EvalInputSpec]) -> Result<(), ReplayError> {
        self.clear_all();
        for spec in specs {
            self.input(&spec.type_id)?.restore(&spec.data)?;
        }
        Ok(())
    }

    /// Clear all live input trackers before a fresh evaluation.
    pub fn clear_all(&self) {
        for input in self.inputs.values() {
            input.clear();
        }
    }

    /// Shorthand for [`Self::clear_all`].
    pub fn clear(&self) {
        self.clear_all();
    }

    /// Return whether any currently-recorded eval input exists.
    pub fn has_inputs(&self) -> bool {
        self.inputs.values().any(|input| !input.is_empty())
    }

    /// Return whether a prior input snapshot has changed.
    ///
    /// A successful `true` is distinct from an error reading an input, so
    /// callers can report filesystem failures instead of silently treating
    /// them as ordinary task invalidation.
    pub fn inputs_changed(&self, specs: &[EvalInputSpec]) -> Result<bool, ReplayError> {
        for spec in specs {
            if self.input(&spec.type_id)?.changed(&spec.data)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Shorthand for [`Self::inputs_changed`].
    pub fn changed(&self, specs: &[EvalInputSpec]) -> Result<bool, ReplayError> {
        self.inputs_changed(specs)
    }

    /// Return whether this manager knows how to validate `type_id`.
    pub fn handles(&self, type_id: &str) -> bool {
        self.inputs.contains_key(type_id)
    }

    fn input(&self, type_id: &str) -> Result<&dyn ErasedEvalInput, ReplayError> {
        self.inputs
            .get(type_id)
            .map(Box::as_ref)
            .ok_or_else(|| ReplayError::Unavailable(format!("Unknown eval input type: {type_id}")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use devenv_core::resource::ReplayError;

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct TestSpec {
        values: Vec<String>,
    }

    #[derive(Default)]
    struct TestInput {
        observed: Mutex<TestSpec>,
        available: Mutex<TestSpec>,
        fail_checks: Mutex<bool>,
    }

    impl TestInput {
        fn with_values(values: &[&str]) -> Self {
            let spec = TestSpec {
                values: values.iter().map(ToString::to_string).collect(),
            };
            Self {
                observed: Mutex::new(spec.clone()),
                available: Mutex::new(spec),
                fail_checks: Mutex::new(false),
            }
        }

        fn set_available(&self, values: &[&str]) {
            *self.available.lock().unwrap() = TestSpec {
                values: values.iter().map(ToString::to_string).collect(),
            };
        }

        fn fail_checks(&self) {
            *self.fail_checks.lock().unwrap() = true;
        }
    }

    impl EvalInput for TestInput {
        type Spec = TestSpec;
        const TYPE_ID: &'static str = "test-input";

        fn snapshot(&self) -> Self::Spec {
            self.observed.lock().unwrap().clone()
        }

        fn is_empty(&self) -> bool {
            self.observed.lock().unwrap().values.is_empty()
        }

        fn changed(&self, spec: &Self::Spec) -> Result<bool, ReplayError> {
            if *self.fail_checks.lock().unwrap() {
                return Err(ReplayError::Input("input could not be read".to_owned()));
            }
            Ok(*self.available.lock().unwrap() != *spec)
        }

        fn restore(&self, spec: &Self::Spec) -> Result<(), ReplayError> {
            self.validate(spec)?;
            *self.observed.lock().unwrap() = spec.clone();
            Ok(())
        }

        fn clear(&self) {
            *self.observed.lock().unwrap() = TestSpec::default();
        }
    }

    #[test]
    fn snapshots_only_non_empty_registered_inputs() {
        let empty = Arc::new(TestInput::default());
        let mut manager = EvalInputManager::new();
        manager.register(empty);
        assert!(manager.snapshot_all().unwrap().is_empty());

        let input = Arc::new(TestInput::with_values(&["one"]));
        let mut manager = EvalInputManager::new();
        manager.register(input);
        let specs = manager.snapshot_all().unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].type_id, TestInput::TYPE_ID);
        assert_eq!(specs[0].data, serde_json::json!({ "values": ["one"] }));
    }

    #[test]
    fn validates_restores_and_clears_inputs() {
        let input = Arc::new(TestInput::with_values(&["one"]));
        let mut manager = EvalInputManager::new();
        manager.register(input.clone());
        let specs = manager.snapshot().unwrap();

        assert!(!manager.changed(&specs).unwrap());
        manager.clear();
        assert!(!manager.has_inputs());

        manager.restore_all(&specs).unwrap();
        assert_eq!(input.snapshot().values, vec!["one"]);
        assert!(manager.has_inputs());

        input.set_available(&["two"]);
        assert!(manager.changed(&specs).unwrap());
        assert!(manager.validate_all(&specs).is_err());

        input.fail_checks();
        assert!(matches!(
            manager.inputs_changed(&specs),
            Err(ReplayError::Input(_))
        ));
    }

    #[test]
    fn unknown_or_malformed_specs_are_not_reusable() {
        let manager = EvalInputManager::new();
        let unknown = EvalInputSpec {
            type_id: "missing".to_owned(),
            data: serde_json::json!({}),
        };
        assert!(manager.changed(&[unknown]).is_err());

        let input = Arc::new(TestInput::with_values(&["one"]));
        let mut manager = EvalInputManager::new();
        manager.register(input);
        let malformed = EvalInputSpec {
            type_id: TestInput::TYPE_ID.to_owned(),
            data: serde_json::json!({ "not-values": [] }),
        };
        assert!(manager.changed(&[malformed]).is_err());
    }
}
