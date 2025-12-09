//! RFC 3339 timestamp wrapper for SystemTime with proper serde serialization.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

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
