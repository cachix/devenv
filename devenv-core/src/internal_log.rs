use num_enum::TryFromPrimitive;
use regex::Regex;
use serde::Deserialize;
use serde_repr::Deserialize_repr;
use std::fmt::{self, Display, Formatter};
use std::sync::LazyLock;

/// Matches the `error:`/`warning:`/`trace:` keyword Nix prefixes onto a log line.
/// Leading ANSI codes and whitespace are ignored.
static NIX_MESSAGE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\x1b\[[0-9;]*m|\s)*(error|warning|trace):").expect("valid regex")
});

/// What a Nix log message actually is, recovered from its text.
///
/// The nix-daemon protocol can't carry a log level, so forwarded build output
/// all arrives at `Error` verbosity. The level is therefore unreliable; we
/// recover the kind from the `error:`/`warning:`/`trace:` keyword the message
/// carries instead.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NixMessageKind {
    /// A real evaluation or build error (`error:`).
    Error,
    /// A warning (`warning:`).
    Warning,
    /// Output from `builtins.trace`.
    Trace,
    /// Anything else — ordinary log output.
    Other,
}

/// Represents Nix's JSON structured log format (--log-format=internal-json).
///
/// See https://github.com/NixOS/nix/blob/a1cc362d9d249b95e4c9ad403f1e6e26ca302413/src/libutil/logging.cc#L173
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum InternalLog {
    Msg {
        level: Verbosity,
        msg: String,
        // Raw message when logging ErrorInfo
        raw_msg: Option<String>,
    },
    Start {
        id: u64,
        level: Verbosity,
        #[serde(rename = "type")]
        typ: ActivityType,
        text: String,
        parent: u64,
        fields: Vec<Field>,
    },
    Stop {
        id: u64,
    },
    Result {
        id: u64,
        #[serde(rename = "type")]
        typ: ResultType,
        fields: Vec<Field>,
    },
    // Possibly deprecated.
    SetPhase {
        phase: String,
    },
}

impl InternalLog {
    // TODO: assumes UTF-8 encoding
    pub fn parse<T>(line: T) -> Option<serde_json::Result<Self>>
    where
        T: AsRef<str>,
    {
        line.as_ref()
            .strip_prefix("@nix ")
            .map(serde_json::from_str)
    }

    /// Classify a message by its leading keyword (see [`NixMessageKind`]).
    pub fn message_kind(&self) -> NixMessageKind {
        let InternalLog::Msg { msg, .. } = self else {
            return NixMessageKind::Other;
        };
        let Some(caps) = NIX_MESSAGE_PREFIX.captures(msg) else {
            return NixMessageKind::Other;
        };
        match &caps[1] {
            "error" => NixMessageKind::Error,
            "warning" => NixMessageKind::Warning,
            "trace" => NixMessageKind::Trace,
            _ => NixMessageKind::Other,
        }
    }
}

/// See https://github.com/NixOS/nix/blob/322d2c767f2a3f8ef2ac3d1ba46c19caf9a1ffce/src/libutil/error.hh#L33-L42
#[derive(
    Copy, Clone, Debug, Default, Deserialize_repr, TryFromPrimitive, PartialEq, Eq, PartialOrd, Ord,
)]
#[repr(u8)]
pub enum Verbosity {
    Error = 0,
    Warn = 1,
    Notice = 2,
    #[default]
    Info = 3,
    Talkative = 4,
    Chatty = 5,
    Debug = 6,
    Vomit = 7,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid verbosity level: {0}")]
pub struct InvalidVerbosity(pub i32);

impl TryFrom<i32> for Verbosity {
    type Error = InvalidVerbosity;

    fn try_from(value: i32) -> Result<Self, <Verbosity as TryFrom<i32>>::Error> {
        u8::try_from(value)
            .ok()
            .and_then(|b| Verbosity::try_from(b).ok())
            .ok_or(InvalidVerbosity(value))
    }
}

/// See https://github.com/NixOS/nix/blob/a5959aa12170fc75cafc9e2416fae9aa67f91e6b/src/libutil/logging.hh#L11-L26
#[derive(
    Copy, Clone, Debug, Deserialize_repr, TryFromPrimitive, PartialEq, Eq, PartialOrd, Ord,
)]
#[repr(u8)]
pub enum ActivityType {
    Unknown = 0,
    CopyPath = 100,
    FileTransfer = 101,
    Realise = 102,
    CopyPaths = 103,
    Builds = 104,
    Build = 105,
    OptimiseStore = 106,
    VerifyPaths = 107,
    Substitute = 108,
    QueryPathInfo = 109,
    PostBuildHook = 110,
    BuildWaiting = 111,
    FetchTree = 112,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid activity type: {0}")]
pub struct InvalidActivityType(pub i32);

impl TryFrom<i32> for ActivityType {
    type Error = InvalidActivityType;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        u8::try_from(value)
            .ok()
            .and_then(|b| ActivityType::try_from(b).ok())
            .ok_or(InvalidActivityType(value))
    }
}

/// See https://github.com/NixOS/nix/blob/a5959aa12170fc75cafc9e2416fae9aa67f91e6b/src/libutil/logging.hh#L28-L38
#[derive(
    Copy, Clone, Debug, Deserialize_repr, TryFromPrimitive, PartialEq, Eq, PartialOrd, Ord,
)]
#[repr(u8)]
pub enum ResultType {
    FileLinked = 100,
    BuildLogLine = 101,
    UntrustedPath = 102,
    CorruptedPath = 103,
    SetPhase = 104,
    Progress = 105,
    SetExpected = 106,
    PostBuildLogLine = 107,
    FetchStatus = 108,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid result type: {0}")]
pub struct InvalidResultType(pub i32);

impl TryFrom<i32> for ResultType {
    type Error = InvalidResultType;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        u8::try_from(value)
            .ok()
            .and_then(|b| ResultType::try_from(b).ok())
            .ok_or(InvalidResultType(value))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Field {
    Int(u64),
    String(String),
}

impl Display for Field {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Field::Int(i) => write!(f, "{i}"),
            Field::String(s) => write!(f, "{s}"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_log_msg() {
        let line = r#"@nix {"action":"msg","level":1,"msg":"hello"}"#;
        let log = InternalLog::parse(line).unwrap().unwrap();
        assert_eq!(
            log,
            InternalLog::Msg {
                level: Verbosity::Warn,
                msg: "hello".to_string(),
                raw_msg: None,
            }
        );
    }

    #[test]
    fn test_parse_log_start() {
        let line = r#"@nix {"action":"start","id":1,"level":3,"type":100,"text":"hello","parent":0,"fields":[]}"#;
        let log = InternalLog::parse(line).unwrap().unwrap();
        assert_eq!(
            log,
            InternalLog::Start {
                id: 1,
                level: Verbosity::Info,
                typ: ActivityType::CopyPath,
                text: "hello".to_string(),
                parent: 0,
                fields: vec![],
            }
        );
    }

    #[test]
    fn test_parse_log_stop() {
        let line = r#"@nix {"action":"stop","id":1}"#;
        let log = InternalLog::parse(line).unwrap().unwrap();
        assert_eq!(log, InternalLog::Stop { id: 1 });
    }

    #[test]
    fn test_parse_log_result() {
        let line = r#"@nix {"action":"result","id":1,"type":101,"fields":["hello"]}"#;
        let log = InternalLog::parse(line).unwrap().unwrap();
        assert_eq!(
            log,
            InternalLog::Result {
                id: 1,
                typ: ResultType::BuildLogLine,
                fields: vec![Field::String("hello".to_string())],
            }
        );
    }

    #[test]
    fn test_parse_invalid_log() {
        let line = r#"@nix {"action":"invalid"}"#;
        assert!(InternalLog::parse(line).unwrap().is_err());
    }

    #[test]
    fn test_parse_non_nix_log() {
        let line = "This is not a Nix log line";
        assert!(InternalLog::parse(line).is_none());
    }

    #[test]
    fn test_verbosity_deserialize() {
        let json = r#"0"#;
        let verbosity: Verbosity = serde_json::from_str(json).unwrap();
        assert_eq!(verbosity, Verbosity::Error);
    }

    /// An `Error`-level `Msg` — the verbosity real errors and mislabeled
    /// daemon lines share.
    fn error_level_msg(msg: &str) -> InternalLog {
        InternalLog::Msg {
            level: Verbosity::Error,
            msg: msg.to_string(),
            raw_msg: None,
        }
    }

    #[test]
    fn message_kind_classifies_by_keyword_ignoring_level() {
        use NixMessageKind::*;

        // The leading keyword decides the kind. The level plays no part: the
        // same `error:` line is an error whether it arrives at Error verbosity
        // (the daemon default) or mislabeled at some other level.
        assert_eq!(error_level_msg("error: boom").message_kind(), Error);
        assert_eq!(
            error_level_msg("\u{1b}[31;1merror:\u{1b}[0m\nsomething went wrong").message_kind(),
            Error
        );
        assert_eq!(
            InternalLog::Msg {
                level: Verbosity::Info,
                msg: "error: still an error at info level".to_string(),
                raw_msg: None,
            }
            .message_kind(),
            Error
        );

        // Traces from `builtins.trace`.
        assert_eq!(
            error_level_msg("trace: from builtins.trace").message_kind(),
            Trace
        );

        // Warnings, colored or not, at any level. Nix colors them magenta
        // (35;1) and the daemon forwards them at Error level — including the
        // restricted-settings notice that untrusted users see.
        assert_eq!(
            error_level_msg("\u{1b}[35;1mwarning:\u{1b}[0m mislabeled at Error").message_kind(),
            Warning
        );
        assert_eq!(
            error_level_msg(
                "\u{1b}[35;1mwarning:\u{1b}[0m ignoring the client-specified setting \
                 'trusted-public-keys', because it is a restricted setting and you \
                 are not a trusted user"
            )
            .message_kind(),
            Warning
        );
        assert_eq!(
            InternalLog::Msg {
                level: Verbosity::Warn,
                msg: "warning: correctly labeled".to_string(),
                raw_msg: None,
            }
            .message_kind(),
            Warning
        );

        // Anchoring: a build line that merely *mentions* the keyword mid-text
        // must not be classified by it.
        assert_eq!(
            error_level_msg("checking for error: handling support... yes").message_kind(),
            Other
        );
        // Ordinary output with no leading keyword.
        assert_eq!(
            error_level_msg("building '/nix/store/...'").message_kind(),
            Other
        );

        // Non-Msg variants are never classified.
        assert_eq!(InternalLog::Stop { id: 1 }.message_kind(), Other);
    }
}
