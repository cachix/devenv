/// Parse a nix.conf into an ordered map of key-value string pairs.
///
/// Closely follows the upstream implementation:
/// https://github.com/NixOS/nix/blob/acb60fc3594edcc54dae9a10d2a0dc3f3b3be0da/src/libutil/config.cc#L104-L161
///
/// Only intended to work on the output of `nix config show`.
/// Therefore, this intentionally leaves out:
///   - includes and !includes
///   - comments
///   - formatting
use indexmap::IndexMap;
use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug)]
pub struct NixConf {
    settings: IndexMap<String, String>,
}

impl NixConf {
    pub fn parse_stdout(input: &[u8]) -> Result<Self, ParseError> {
        let input = String::from_utf8_lossy(input);
        Self::parse_str(&input)
    }

    /// Parse a string into an ordered map of key-value string pairs.
    pub fn parse_str(input: &str) -> Result<Self, ParseError> {
        let mut settings = IndexMap::new();

        for mut line in input.lines() {
            // Trim comments
            if let Some(pos) = line.find('#') {
                line = &line[..pos];
            }

            if line.trim().is_empty() {
                continue;
            }

            let mut tokens = line.split_whitespace().collect::<Vec<_>>();
            tokens.retain(|t| !t.is_empty());

            if tokens.is_empty() {
                continue;
            }

            if tokens.len() < 2 {
                return Err(ParseError::IllegalConfiguration(line.to_string()));
            }

            // Skip includes if they make it into the input
            match tokens[0] {
                "include" | "!include" => continue,
                _ => {}
            }

            if tokens[1] != "=" {
                return Err(ParseError::IllegalConfiguration(line.to_string()));
            }

            let name = tokens[0];
            let value = tokens[2..].join(" ");

            settings.insert(name.to_string(), value);
        }

        Ok(Self { settings })
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.settings.get(key)
    }
}

#[derive(Debug, Diagnostic, Error)]
pub enum ParseError {
    #[error("illegal configuration line '{0}'")]
    IllegalConfiguration(String),
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() {
        let input = r#"
            # This is a comment
            include /etc/nixos/hardware-configuration.nix
            !include /etc/nixos/hardware-configuration.nix
            single = foo
            space  =  foo   bar 
            list = foo bar baz
            comment = foo # comment
            tab =	 foo 
        "#;
        let nix_conf = NixConf::parse_str(input).unwrap();
        assert_eq!(nix_conf.get("single"), Some(&"foo".into()));
        assert_eq!(nix_conf.get("space"), Some(&"foo bar".into()));
        assert_eq!(nix_conf.get("list"), Some(&"foo bar baz".into()));
        assert_eq!(nix_conf.get("comment"), Some(&"foo".into()));
        assert_eq!(nix_conf.get("tab"), Some(&"foo".into()));
    }
}
