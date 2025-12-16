//! Bridge from serde to valuable.
//!
//! This module provides [`SerdeValue`], a wrapper that implements [`Valuable`]
//! for [`serde_json::Value`], allowing serde-serialized data to be passed through
//! tracing while respecting serde's rename attributes.

use serde::Serialize;
use valuable::{Listable, Mappable, Valuable, Value, Visit};

/// A wrapper around [`serde_json::Value`] that implements [`Valuable`].
///
/// This allows you to serialize a type using serde (respecting `#[serde(rename_all)]`
/// and other attributes) and then pass it through tracing's valuable system.
///
/// # Example
///
/// ```ignore
/// use devenv_activity::serde_valuable::SerdeValue;
///
/// #[derive(Serialize)]
/// #[serde(rename_all = "lowercase")]
/// enum MyEnum { Variant1, Variant2 }
///
/// let value = serde_json::to_value(&MyEnum::Variant1).unwrap();
/// tracing::trace!(event = SerdeValue(value).as_value());
/// // Output will have "variant1" not "Variant1"
/// ```
pub struct SerdeValue(pub serde_json::Value);

impl SerdeValue {
    /// Serialize a value using serde and wrap it for use with valuable/tracing.
    pub fn from_serialize<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        serde_json::to_value(value).map(SerdeValue)
    }
}

impl Valuable for SerdeValue {
    fn as_value(&self) -> Value<'_> {
        match &self.0 {
            serde_json::Value::Null => Value::Unit,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::I64(i)
                } else if let Some(u) = n.as_u64() {
                    Value::U64(u)
                } else if let Some(f) = n.as_f64() {
                    Value::F64(f)
                } else {
                    Value::Unit
                }
            }
            serde_json::Value::String(s) => Value::String(s.as_str()),
            // For complex types, return self as the container
            serde_json::Value::Array(_) => Value::Listable(self),
            serde_json::Value::Object(_) => Value::Mappable(self),
        }
    }

    fn visit(&self, visit: &mut dyn Visit) {
        match &self.0 {
            serde_json::Value::Null => visit.visit_value(Value::Unit),
            serde_json::Value::Bool(b) => visit.visit_value(Value::Bool(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    visit.visit_value(Value::I64(i));
                } else if let Some(u) = n.as_u64() {
                    visit.visit_value(Value::U64(u));
                } else if let Some(f) = n.as_f64() {
                    visit.visit_value(Value::F64(f));
                }
            }
            serde_json::Value::String(s) => visit.visit_value(Value::String(s.as_str())),
            serde_json::Value::Array(arr) => {
                for item in arr {
                    visit.visit_value(SerdeValue(item.clone()).as_value());
                }
            }
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    visit.visit_entry(
                        Value::String(key.as_str()),
                        SerdeValue(value.clone()).as_value(),
                    );
                }
            }
        }
    }
}

impl Listable for SerdeValue {
    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            serde_json::Value::Array(arr) => (arr.len(), Some(arr.len())),
            _ => (0, Some(0)),
        }
    }
}

impl Mappable for SerdeValue {
    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            serde_json::Value::Object(map) => (map.len(), Some(map.len())),
            _ => (0, Some(0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    #[serde(rename_all = "lowercase")]
    enum TestEnum {
        VariantOne,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    struct TestStruct {
        field_name: String,
        nested_value: i32,
    }

    #[test]
    fn test_enum_lowercase() {
        let serde_value = SerdeValue::from_serialize(&TestEnum::VariantOne).unwrap();
        assert_eq!(
            serde_value.0,
            serde_json::Value::String("variantone".to_string())
        );
    }

    #[test]
    fn test_struct_snake_case() {
        let s = TestStruct {
            field_name: "test".to_string(),
            nested_value: 42,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("field_name").is_some());
        assert!(json.get("nested_value").is_some());
    }
}
