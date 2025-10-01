use serde::Deserialize;
use serde_repr::Deserialize_repr;
use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};

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

    pub fn filter_by_level(&self, target_log_level: Verbosity) -> Option<&Self> {
        match self {
            // A lot of build messages are tagged as level 0 (Error), making it difficult
            // to filter things out. Our hunch is that these messages are coming from the
            // nix daemon.
            InternalLog::Msg { level, .. }
                if *level == Verbosity::Error
                    && (self.is_nix_error() || self.is_builtin_trace()) =>
            {
                Some(self)
            }

            InternalLog::Msg { level, .. }
                if *level > Verbosity::Error && *level <= target_log_level =>
            {
                Some(self)
            }

            // The log levels are also broken for activity messages.
            InternalLog::Start {
                level: Verbosity::Error,
                ..
            } => {
                if target_log_level >= Verbosity::Info {
                    Some(self)
                } else {
                    None
                }
            }

            InternalLog::Start { level, .. } if *level <= target_log_level => Some(self),

            InternalLog::Result {
                typ: ResultType::BuildLogLine,
                ..
            } if target_log_level >= Verbosity::Info => Some(self),
            _ => None,
        }
    }

    /// Extract or format a human-readable message from the log.
    ///
    /// Reference for activity messages:
    /// https://github.com/NixOS/nix/blob/ff00eebb16fc4c0fd4cebf0cbfc63c471e3c4abd/src/libmain/progress-bar.cc#L177
    pub fn get_msg(&self) -> Option<Cow<'_, String>> {
        use std::fmt::Write;

        match self {
            InternalLog::Msg { msg, .. } => Some(Cow::Borrowed(msg)),
            InternalLog::Start {
                typ: ActivityType::Substitute,
                fields,
                ..
            } => fields.first().zip(fields.get(1)).and_then(|(path, sub)| {
                if let (Field::String(path), Field::String(sub)) = (path, sub) {
                    let name = store_path_to_name(path);
                    let action = if sub.starts_with("local") {
                        "copying"
                    } else {
                        "fetching"
                    };
                    Some(Cow::Owned(format!("{action} {name} from {sub}")))
                } else {
                    None
                }
            }),

            InternalLog::Start {
                typ: ActivityType::Build,
                fields,
                ..
            } => {
                if let Some(Field::String(name)) = fields.first() {
                    let name = name.strip_suffix(".drv").unwrap_or(name);
                    let mut msg = format!("building {name}");
                    if let Some(Field::String(machine_name)) = fields.get(1) {
                        write!(msg, " on {machine_name}").ok();
                    }
                    Some(Cow::Owned(msg))
                } else {
                    None
                }
            }

            InternalLog::Start {
                typ: ActivityType::QueryPathInfo,
                fields,
                ..
            } => fields.first().zip(fields.get(1)).and_then(|(path, sub)| {
                if let (Field::String(path), Field::String(sub)) = (path, sub) {
                    let name = store_path_to_name(path);
                    Some(Cow::Owned(format!("querying {name} on {sub}")))
                } else {
                    None
                }
            }),

            InternalLog::Result {
                typ: ResultType::BuildLogLine,
                fields,
                ..
            } => {
                let mut msg = String::new();
                for field in fields {
                    writeln!(msg, "{field}").ok();
                }
                Some(Cow::Owned(msg.trim_end().to_string()))
            }
            _ => None,
        }
    }

    /// Check if the log is an actual error message.
    ///
    /// In additional to checking the verbosity level of the message, we look for the `error:` prefix in the message.
    /// Most messages during the builds (probably from the nix-daemon) are incorrectly logged as errors.
    pub fn is_nix_error(&self) -> bool {
        if let InternalLog::Msg {
            level: Verbosity::Error,
            msg,
            ..
        } = self
            && msg.starts_with("\u{1b}[31;1merror:")
        {
            return true;
        }

        false
    }

    /// Check if the log is a trace message from `builtins.trace`.
    pub fn is_builtin_trace(&self) -> bool {
        if let InternalLog::Msg {
            level: Verbosity::Error,
            msg,
            ..
        } = self
            && msg.starts_with("trace:")
        {
            return true;
        }

        false
    }
}

/// See https://github.com/NixOS/nix/blob/322d2c767f2a3f8ef2ac3d1ba46c19caf9a1ffce/src/libutil/error.hh#L33-L42
#[derive(Copy, Clone, Debug, Default, Deserialize_repr, PartialEq, Eq, PartialOrd, Ord)]
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

/// See https://github.com/NixOS/nix/blob/a5959aa12170fc75cafc9e2416fae9aa67f91e6b/src/libutil/logging.hh#L11-L26
#[derive(Copy, Clone, Debug, Deserialize_repr, PartialEq, Eq, PartialOrd, Ord)]
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

/// See https://github.com/NixOS/nix/blob/a5959aa12170fc75cafc9e2416fae9aa67f91e6b/src/libutil/logging.hh#L28-L38
#[derive(Copy, Clone, Debug, Deserialize_repr, PartialEq, Eq, PartialOrd, Ord)]
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

fn store_path_to_name(path: &str) -> &str {
    if let Some((_, name)) = path.split_once('-') {
        name
    } else {
        path
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

    #[test]
    // Ensure that only messages containing the prefix `error:` are detected as error messages.
    // See `is_nix_error` for more details.
    fn test_is_nix_error() {
        let log = InternalLog::Msg {
            level: Verbosity::Error,
            msg: "\u{1b}[31;1merror:\u{1b}[0m\nsomething went wrong".to_string(),
            raw_msg: None,
        };
        assert!(log.is_nix_error());
    }

    #[test]
    // Ensure we don't interpret non-error messages as errors.
    // See `is_nix_error` for more details.
    fn test_is_nix_error_misleveled_msgs() {
        let log = InternalLog::Msg {
            level: Verbosity::Error,
            msg: "not an error".to_string(),
            raw_msg: None,
        };
        assert!(!log.is_nix_error());
    }
}
