//! RFC 3339 timestamp wrapper for SystemTime with proper serde serialization.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use valuable::Valuable;

/// RFC 3339 timestamp wrapper for SystemTime with proper serde serialization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(pub SystemTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(SystemTime::now())
    }
}

impl From<SystemTime> for Timestamp {
    fn from(time: SystemTime) -> Self {
        Self(time)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&humantime::format_rfc3339_nanos(self.0).to_string())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        humantime::parse_rfc3339(&s)
            .map(Timestamp)
            .map_err(D::Error::custom)
    }
}

// NOTE: SystemTime doesn't implement Valuable.
// We also can't output a string because Value::String requires a &'a str tied to self's lifetime.
// Using Structable with visit_named_fields lets us create a temporary string that the visitor consumes immediately.
impl Valuable for Timestamp {
    fn as_value(&self) -> valuable::Value<'_> {
        valuable::Value::Structable(self)
    }

    fn visit(&self, visit: &mut dyn valuable::Visit) {
        let formatted = humantime::format_rfc3339_nanos(self.0).to_string();
        visit.visit_named_fields(&valuable::NamedValues::new(
            &[valuable::NamedField::new("timestamp")],
            &[valuable::Value::String(&formatted)],
        ));
    }
}

impl valuable::Structable for Timestamp {
    fn definition(&self) -> valuable::StructDef<'_> {
        static FIELDS: &[valuable::NamedField<'static>] = &[valuable::NamedField::new("timestamp")];
        valuable::StructDef::new_static("Timestamp", valuable::Fields::Named(FIELDS))
    }
}
