use serde::Deserialize;

/// See https://github.com/NixOS/nix/blob/a1cc362d9d249b95e4c9ad403f1e6e26ca302413/src/libutil/logging.cc#L173
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum NixInternalLog {
    Msg {
        level: u8,
        msg: String,
        // Raw message when logging ErrorInfo
        raw_msg: Option<String>,
    },
    Start {
        id: u64,
        level: u8,
        #[serde(rename = "type")]
        typ: u8,
        text: String,
        parent: u64,
        fields: Vec<NixField>,
    },
    Stop {
        id: u64,
    },
    Result {
        id: u64,
        #[serde(rename = "type")]
        typ: u8,
        fields: Vec<NixField>,
    },
    // Possibly deprecated.
    SetPhase {
        phase: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NixField {
    Int(u64),
    String(String),
}

// TODO: assumes UTF-8 encoding
pub fn parse_log_line<T>(line: T) -> Option<serde_json::Result<NixInternalLog>>
where
    T: AsRef<str>,
{
    line.as_ref()
        .strip_prefix("@nix ")
        .map(serde_json::from_str)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_log_msg() {
        let line = r#"@nix {"action":"msg","level":1,"msg":"hello"}"#;
        let log = parse_log_line(line).unwrap().unwrap();
        assert_eq!(
            log,
            NixInternalLog::Msg {
                level: 1,
                msg: "hello".to_string(),
                raw_msg: None,
            }
        );
    }

    #[test]
    fn test_parse_log_start() {
        let line = r#"@nix {"action":"start","id":1,"level":1,"type":1,"text":"hello","parent":0,"fields":[]}"#;
        let log = parse_log_line(line).unwrap().unwrap();
        assert_eq!(
            log,
            NixInternalLog::Start {
                id: 1,
                level: 1,
                typ: 1,
                text: "hello".to_string(),
                parent: 0,
                fields: vec![],
            }
        );
    }

    #[test]
    fn test_parse_log_stop() {
        let line = r#"@nix {"action":"stop","id":1}"#;
        let log = parse_log_line(line).unwrap().unwrap();
        assert_eq!(log, NixInternalLog::Stop { id: 1 });
    }

    #[test]
    fn test_parse_log_result() {
        let line = r#"@nix {"action":"result","id":1,"type":1,"fields":[]}"#;
        let log = parse_log_line(line).unwrap().unwrap();
        assert_eq!(
            log,
            NixInternalLog::Result {
                id: 1,
                typ: 1,
                fields: vec![],
            }
        );
    }

    #[test]
    fn test_parse_invalid_log() {
        let line = r#"@nix {"action":"invalid"}"#;
        assert!(parse_log_line(line).unwrap().is_err());
    }

    #[test]
    fn test_parse_non_nix_log() {
        let line = "This is not a Nix log line";
        assert!(parse_log_line(line).is_none());
    }
}
