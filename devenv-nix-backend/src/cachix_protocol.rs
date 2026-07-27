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

/// Daemon-to-client replies, parsed from a raw [`DaemonMessage`].
///
/// Mirrors `DaemonMessage` in `Cachix.Daemon.Protocol`. The push event
/// stream keeps its dedicated parser ([`PushEvent`]); `PushEvent` here just
/// marks the tag so control-connection readers can skip those messages.
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonReply {
    Pong,
    Exit(DaemonExitStatus),
    Error(DaemonErrorMessage),
    PushEvent,
    DiagnosticsResult,
    Unknown,
}

/// Exit status the daemon reports in its `DaemonExit` farewell message.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonExitStatus {
    pub exit_code: i64,
    #[serde(default)]
    pub exit_message: Option<String>,
}

/// Error messages the daemon can send to the client.
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonErrorMessage {
    /// The daemon rejected a command, e.g. `ClientStop` when remote stop is
    /// disabled (`--no-remote-stop`).
    UnsupportedCommand(String),
    Unknown,
}

impl DaemonReply {
    pub fn parse(msg: &DaemonMessage) -> DaemonReply {
        match msg.tag.as_str() {
            "DaemonPong" => DaemonReply::Pong,
            "DaemonExit" => serde_json::from_value(msg.contents.clone())
                .map(DaemonReply::Exit)
                .unwrap_or(DaemonReply::Unknown),
            "DaemonError" => DaemonReply::Error(DaemonErrorMessage::parse(&msg.contents)),
            "DaemonPushEvent" => DaemonReply::PushEvent,
            "DaemonDiagnosticsResult" => DaemonReply::DiagnosticsResult,
            _ => DaemonReply::Unknown,
        }
    }
}

impl DaemonErrorMessage {
    fn parse(contents: &Value) -> DaemonErrorMessage {
        match contents.get("tag").and_then(Value::as_str) {
            Some("UnsupportedCommand") => DaemonErrorMessage::UnsupportedCommand(scalar_string(
                contents.get("contents").unwrap_or(&Value::Null),
            )),
            _ => DaemonErrorMessage::Unknown,
        }
    }
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

/// Messages the client sends to the daemon.
///
/// Mirrors `ClientMessage` in `Cachix.Daemon.Protocol`. Serde's adjacent
/// tagging matches Aeson's `TaggedObject` here: unit variants serialize as
/// a bare `{"tag":...}` with no `contents` key.
#[derive(Debug, Serialize)]
#[serde(tag = "tag", content = "contents")]
pub enum ClientMessage {
    ClientPushRequest(PushRequest),
    /// Asks the daemon to shut down gracefully: it drains its push queue,
    /// replies with `DaemonExit`, and exits. Honored when the daemon runs
    /// with remote stop enabled (the default for `cachix daemon run`).
    ClientStop,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub store_paths: Vec<String>,
    pub subscribe_to_updates: bool,
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
        let req = ClientMessage::ClientPushRequest(PushRequest {
            store_paths: vec!["/nix/store/a".into(), "/nix/store/b".into()],
            subscribe_to_updates: true,
        });
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(json["tag"], "ClientPushRequest");
        assert_eq!(json["contents"]["storePaths"][0], "/nix/store/a");
        assert_eq!(json["contents"]["storePaths"][1], "/nix/store/b");
        assert_eq!(json["contents"]["subscribeToUpdates"], true);
    }

    #[test]
    fn client_stop_serializes_as_bare_tag() {
        // Aeson's TaggedObject encodes nullary constructors without a
        // `contents` key; pin the exact bytes so serde stays in step.
        let json = serde_json::to_string(&ClientMessage::ClientStop).unwrap();
        assert_eq!(json, r#"{"tag":"ClientStop"}"#);
    }

    fn parse_reply(json: serde_json::Value) -> DaemonReply {
        let msg: DaemonMessage = serde_json::from_value(json).expect("DaemonMessage");
        DaemonReply::parse(&msg)
    }

    #[test]
    fn daemon_exit_clean() {
        let reply = parse_reply(json!({
            "tag": "DaemonExit",
            "contents": { "exitCode": 0, "exitMessage": null },
        }));
        assert_eq!(
            reply,
            DaemonReply::Exit(DaemonExitStatus {
                exit_code: 0,
                exit_message: None,
            })
        );
    }

    #[test]
    fn daemon_exit_with_message() {
        let reply = parse_reply(json!({
            "tag": "DaemonExit",
            "contents": { "exitCode": 3, "exitMessage": "push failure" },
        }));
        assert_eq!(
            reply,
            DaemonReply::Exit(DaemonExitStatus {
                exit_code: 3,
                exit_message: Some("push failure".into()),
            })
        );
    }

    #[test]
    fn daemon_error_unsupported_command() {
        let reply = parse_reply(json!({
            "tag": "DaemonError",
            "contents": {
                "tag": "UnsupportedCommand",
                "contents": "Remote stop is disabled on this daemon",
            },
        }));
        assert_eq!(
            reply,
            DaemonReply::Error(DaemonErrorMessage::UnsupportedCommand(
                "Remote stop is disabled on this daemon".into()
            ))
        );
    }

    #[test]
    fn daemon_pong_no_contents() {
        assert_eq!(
            parse_reply(json!({ "tag": "DaemonPong" })),
            DaemonReply::Pong
        );
    }

    #[test]
    fn daemon_reply_unknown_tag() {
        assert_eq!(
            parse_reply(json!({ "tag": "DaemonSomethingNew", "contents": [1, 2] })),
            DaemonReply::Unknown
        );
    }

    #[test]
    fn daemon_exit_malformed_contents_yields_unknown() {
        assert_eq!(
            parse_reply(json!({ "tag": "DaemonExit", "contents": "nope" })),
            DaemonReply::Unknown
        );
    }
}
