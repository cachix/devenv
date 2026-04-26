//! Wire protocol for the cachix daemon socket.
//!
//! Aeson's default `TaggedObject` sum encoding shapes `contents` differently
//! per arity (no `contents` for nullary, scalar for unary, array for n-ary).
//! See `cachix/src/Cachix/Daemon/PROTOCOL.md`. Golden tests below pin the
//! exact wire bytes for each variant.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct DaemonMessage {
    pub tag: String,
    #[serde(default)]
    pub contents: Value,
}

#[derive(Debug, Deserialize)]
pub struct PushEventEnvelope {
    #[serde(rename = "eventTimestamp")]
    #[allow(dead_code)]
    pub timestamp: String,
    #[serde(rename = "eventPushId")]
    #[allow(dead_code)]
    pub push_id: String,
    #[serde(rename = "eventMessage")]
    pub message: DaemonMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushEvent {
    PushStarted,
    StorePathAttempt {
        path: String,
        nar_size: u64,
        retry_count: u64,
    },
    StorePathProgress {
        path: String,
        current_bytes: u64,
        delta_bytes: u64,
    },
    StorePathDone {
        path: String,
    },
    StorePathFailed {
        path: String,
        reason: String,
    },
    /// Emitted instead of `StorePathDone` when the path is already in the cache.
    StorePathSkipped {
        path: String,
    },
    PushFinished,
    Unknown,
}

impl PushEvent {
    pub fn parse(msg: &DaemonMessage) -> PushEvent {
        match msg.tag.as_str() {
            "PushStarted" => PushEvent::PushStarted,
            "PushFinished" => PushEvent::PushFinished,
            "PushStorePathAttempt" => {
                let Some(arr) = msg.contents.as_array() else {
                    return PushEvent::Unknown;
                };
                PushEvent::StorePathAttempt {
                    path: str_at(arr, 0).to_string(),
                    nar_size: arr.get(1).and_then(Value::as_u64).unwrap_or(0),
                    retry_count: arr
                        .get(2)
                        .and_then(|v| v.get("retryCount"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                }
            }
            "PushStorePathProgress" => {
                let Some(arr) = msg.contents.as_array() else {
                    return PushEvent::Unknown;
                };
                PushEvent::StorePathProgress {
                    path: str_at(arr, 0).to_string(),
                    current_bytes: arr.get(1).and_then(Value::as_u64).unwrap_or(0),
                    delta_bytes: arr.get(2).and_then(Value::as_u64).unwrap_or(0),
                }
            }
            "PushStorePathDone" => PushEvent::StorePathDone {
                path: scalar_string(&msg.contents),
            },
            "PushStorePathSkipped" => PushEvent::StorePathSkipped {
                path: scalar_string(&msg.contents),
            },
            "PushStorePathFailed" => {
                let Some(arr) = msg.contents.as_array() else {
                    return PushEvent::Unknown;
                };
                PushEvent::StorePathFailed {
                    path: str_at(arr, 0).to_string(),
                    reason: arr
                        .get(1)
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                        .to_string(),
                }
            }
            _ => PushEvent::Unknown,
        }
    }
}

fn str_at(arr: &[Value], i: usize) -> &str {
    arr.get(i).and_then(Value::as_str).unwrap_or("")
}

/// Aeson emits unary `contents` as a scalar. Accept a one-element array
/// too — PROTOCOL.md documents that shape and clients may hand-roll it.
fn scalar_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(s) = v.as_array().and_then(|a| a.first()).and_then(Value::as_str) {
        return s.to_string();
    }
    String::new()
}

#[derive(Debug, Serialize)]
pub struct ClientPushRequest {
    pub tag: String,
    pub contents: PushRequestContents,
}

#[derive(Debug, Serialize)]
pub struct PushRequestContents {
    #[serde(rename = "storePaths")]
    pub store_paths: Vec<String>,
    #[serde(rename = "subscribeToUpdates")]
    pub subscribe_to_updates: bool,
}

impl ClientPushRequest {
    pub fn new(store_paths: Vec<String>, subscribe: bool) -> Self {
        Self {
            tag: "ClientPushRequest".to_string(),
            contents: PushRequestContents {
                store_paths,
                subscribe_to_updates: subscribe,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Wrap an inner event in the envelope the daemon actually sends, so
    /// tests cover the full parse pipeline (DaemonMessage → envelope →
    /// event), not just the inner `parse`.
    fn envelope_json(inner: serde_json::Value) -> String {
        json!({
            "tag": "DaemonPushEvent",
            "contents": {
                "eventTimestamp": "2025-11-07T12:34:56.789123Z",
                "eventPushId": "550e8400-e29b-41d4-a716-446655440000",
                "eventMessage": inner,
            }
        })
        .to_string()
    }

    fn parse(json: &str) -> PushEvent {
        let msg: DaemonMessage = serde_json::from_str(json).expect("DaemonMessage");
        assert_eq!(msg.tag, "DaemonPushEvent");
        let envelope: PushEventEnvelope = serde_json::from_value(msg.contents).expect("envelope");
        PushEvent::parse(&envelope.message)
    }

    // --- Wire-format goldens. These shapes match what the cachix daemon
    //     actually emits via Aeson; verified by running runghc against
    //     Cachix.Daemon.Types.PushEvent.

    #[test]
    fn started_no_contents() {
        let json = envelope_json(json!({ "tag": "PushStarted" }));
        assert_eq!(parse(&json), PushEvent::PushStarted);
    }

    #[test]
    fn finished_no_contents() {
        let json = envelope_json(json!({ "tag": "PushFinished" }));
        assert_eq!(parse(&json), PushEvent::PushFinished);
    }

    #[test]
    fn done_scalar_contents() {
        // Real daemon shape: contents is a string, not [string].
        let json = envelope_json(json!({
            "tag": "PushStorePathDone",
            "contents": "/nix/store/abc",
        }));
        assert_eq!(
            parse(&json),
            PushEvent::StorePathDone {
                path: "/nix/store/abc".into(),
            }
        );
    }

    #[test]
    fn done_array_fallback_for_protocol_md() {
        // PROTOCOL.md documents an array form. We accept it for
        // forward/backward compat even though Aeson doesn't emit it.
        let json = envelope_json(json!({
            "tag": "PushStorePathDone",
            "contents": ["/nix/store/abc"],
        }));
        assert_eq!(
            parse(&json),
            PushEvent::StorePathDone {
                path: "/nix/store/abc".into(),
            }
        );
    }

    #[test]
    fn skipped_scalar_contents() {
        let json = envelope_json(json!({
            "tag": "PushStorePathSkipped",
            "contents": "/nix/store/abc",
        }));
        assert_eq!(
            parse(&json),
            PushEvent::StorePathSkipped {
                path: "/nix/store/abc".into(),
            }
        );
    }

    #[test]
    fn attempt_array_contents() {
        let json = envelope_json(json!({
            "tag": "PushStorePathAttempt",
            "contents": ["/nix/store/abc", 1024, { "retryCount": 0 }],
        }));
        assert_eq!(
            parse(&json),
            PushEvent::StorePathAttempt {
                path: "/nix/store/abc".into(),
                nar_size: 1024,
                retry_count: 0,
            }
        );
    }

    #[test]
    fn attempt_with_retry() {
        let json = envelope_json(json!({
            "tag": "PushStorePathAttempt",
            "contents": ["/p", 2048, { "retryCount": 3 }],
        }));
        match parse(&json) {
            PushEvent::StorePathAttempt { retry_count, .. } => assert_eq!(retry_count, 3),
            other => panic!("Expected StorePathAttempt, got {:?}", other),
        }
    }

    #[test]
    fn progress_array_contents() {
        let json = envelope_json(json!({
            "tag": "PushStorePathProgress",
            "contents": ["/p", 512, 128],
        }));
        assert_eq!(
            parse(&json),
            PushEvent::StorePathProgress {
                path: "/p".into(),
                current_bytes: 512,
                delta_bytes: 128,
            }
        );
    }

    #[test]
    fn failed_array_contents() {
        let json = envelope_json(json!({
            "tag": "PushStorePathFailed",
            "contents": ["/p", "HTTP 403"],
        }));
        assert_eq!(
            parse(&json),
            PushEvent::StorePathFailed {
                path: "/p".into(),
                reason: "HTTP 403".into(),
            }
        );
    }

    #[test]
    fn unknown_tag_yields_unknown() {
        let json = envelope_json(json!({
            "tag": "PushSomethingNew",
            "contents": "anything",
        }));
        assert_eq!(parse(&json), PushEvent::Unknown);
    }

    #[test]
    fn malformed_attempt_yields_unknown() {
        // Scalar where array required: Unknown, not panic. Defends
        // against daemon bugs / version skew.
        let json = envelope_json(json!({
            "tag": "PushStorePathAttempt",
            "contents": "not-an-array",
        }));
        assert_eq!(parse(&json), PushEvent::Unknown);
    }

    #[test]
    fn client_push_request_serializes_correctly() {
        let req = ClientPushRequest::new(vec!["/nix/store/a".into(), "/nix/store/b".into()], true);
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(json["tag"], "ClientPushRequest");
        assert_eq!(json["contents"]["storePaths"][0], "/nix/store/a");
        assert_eq!(json["contents"]["storePaths"][1], "/nix/store/b");
        assert_eq!(json["contents"]["subscribeToUpdates"], true);
    }
}
