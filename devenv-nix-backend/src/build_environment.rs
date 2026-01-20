//! Pure Rust implementation of BuildEnvironment parsing and bash conversion.
//!
//! This allows reading the cached -env JSON directly from the Nix store,
//! bypassing the expensive FFI call to get_dev_environment.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A captured build environment from a Nix shell derivation.
#[derive(Debug, Deserialize)]
pub struct BuildEnvironment {
    pub variables: HashMap<String, Variable>,
    #[serde(rename = "bashFunctions", default)]
    pub bash_functions: HashMap<String, String>,
}

/// A bash variable with its type and value.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Variable {
    Var {
        value: String,
    },
    Exported {
        value: String,
    },
    Array {
        value: Vec<String>,
    },
    Associative {
        value: HashMap<String, String>,
    },
    #[serde(other)]
    Unknown,
}

/// Variables that should be filtered out (sandbox-specific).
const IGNORED_VARS: &[&str] = &[
    "BASHOPTS",
    "HOME",
    "NIX_BUILD_TOP",
    "NIX_ENFORCE_PURITY",
    "NIX_LOG_FD",
    "NIX_REMOTE",
    "PPID",
    "SHELLOPTS",
    "SSL_CERT_FILE",
    "TEMP",
    "TEMPDIR",
    "TERM",
    "TMP",
    "TMPDIR",
    "TZ",
    "UID",
];

impl BuildEnvironment {
    /// Parse a BuildEnvironment from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Read and parse a BuildEnvironment from a JSON file.
    pub fn from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Self::from_json(&json).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Convert the environment to a sourceable bash script.
    pub fn to_bash(&self) -> String {
        let mut out = String::new();

        for (name, var) in &self.variables {
            if IGNORED_VARS.contains(&name.as_str()) {
                continue;
            }

            match var {
                Variable::Var { value } => {
                    out.push_str(&format!("{}={}\n", name, escape_shell_arg(value)));
                }
                Variable::Exported { value } => {
                    out.push_str(&format!("{}={}\n", name, escape_shell_arg(value)));
                    out.push_str(&format!("export {}\n", name));
                }
                Variable::Array { value } => {
                    out.push_str(&format!("declare -a {}=(", name));
                    for v in value {
                        out.push_str(&escape_shell_arg(v));
                        out.push(' ');
                    }
                    out.push_str(")\n");
                }
                Variable::Associative { value } => {
                    out.push_str(&format!("declare -A {}=(", name));
                    for (k, v) in value {
                        out.push_str(&format!(
                            "[{}]={} ",
                            escape_shell_arg(k),
                            escape_shell_arg(v)
                        ));
                    }
                    out.push_str(")\n");
                }
                Variable::Unknown => {}
            }
        }

        for (name, body) in &self.bash_functions {
            out.push_str(&format!("{} ()\n{{\n{}}}\n", name, body));
        }

        out
    }

    /// Convert to a full shell activation script.
    ///
    /// This wraps the raw bash output with:
    /// - Saving PATH and XDG_DATA_DIRS before applying the environment
    /// - Restoring them after (appending to preserve system paths)
    /// - Evaluating shellHook
    ///
    /// Matches `nix develop` behavior from nix/src/nix/develop.cc.
    pub fn to_activation_script(&self) -> String {
        const SAVED_VARS: &[&str] = &["PATH", "XDG_DATA_DIRS"];

        let mut out = String::new();

        // Save current values
        for var in SAVED_VARS {
            out.push_str(&format!("{var}=${{{var}:-}}\n"));
            out.push_str(&format!("nix_saved_{var}=\"${var}\"\n"));
        }

        // Environment variables and functions
        out.push_str(&self.to_bash());

        // Restore saved values (append to preserve system paths)
        for var in SAVED_VARS {
            out.push_str(&format!(
                "{var}=\"${var}${{nix_saved_{var}:+:$nix_saved_{var}}}\"\n"
            ));
        }

        // Evaluate shellHook
        out.push_str("\neval \"${shellHook:-}\"\n");
        out
    }
}

/// Escape a string for use as a bash argument (single-quoted).
fn escape_shell_arg(s: &str) -> String {
    let mut r = String::with_capacity(s.len() + 2);
    r.push('\'');
    for c in s.chars() {
        if c == '\'' {
            r.push_str("'\\''");
        } else {
            r.push(c);
        }
    }
    r.push('\'');
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_shell_arg() {
        assert_eq!(escape_shell_arg("hello"), "'hello'");
        assert_eq!(escape_shell_arg("it's"), "'it'\\''s'");
        assert_eq!(escape_shell_arg(""), "''");
        assert_eq!(escape_shell_arg("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn test_from_json() {
        let json = r#"{
            "variables": {
                "PATH": {"type": "exported", "value": "/nix/store/bin"},
                "name": {"type": "var", "value": "test"},
                "buildInputs": {"type": "array", "value": ["pkg1", "pkg2"]},
                "meta": {"type": "associative", "value": {"name": "foo", "version": "1.0"}}
            },
            "bashFunctions": {
                "genericBuild": "echo building"
            }
        }"#;

        let env = BuildEnvironment::from_json(json).expect("parse failed");
        assert_eq!(env.variables.len(), 4);
        assert_eq!(env.bash_functions.len(), 1);

        match &env.variables["PATH"] {
            Variable::Exported { value } => assert_eq!(value, "/nix/store/bin"),
            _ => panic!("expected exported"),
        }

        match &env.variables["buildInputs"] {
            Variable::Array { value } => assert_eq!(value, &vec!["pkg1", "pkg2"]),
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn test_to_bash() {
        let json = r#"{
            "variables": {
                "PATH": {"type": "exported", "value": "/bin"},
                "FOO": {"type": "var", "value": "bar"}
            },
            "bashFunctions": {}
        }"#;

        let env = BuildEnvironment::from_json(json).expect("parse failed");
        let bash = env.to_bash();

        assert!(bash.contains("PATH='/bin'"));
        assert!(bash.contains("export PATH"));
        assert!(bash.contains("FOO='bar'"));
        assert!(!bash.contains("export FOO"));
    }

    #[test]
    fn test_ignored_vars() {
        let json = r#"{
            "variables": {
                "HOME": {"type": "exported", "value": "/homeless-shelter"},
                "PATH": {"type": "exported", "value": "/bin"}
            },
            "bashFunctions": {}
        }"#;

        let env = BuildEnvironment::from_json(json).expect("parse failed");
        let bash = env.to_bash();

        assert!(!bash.contains("HOME="));
        assert!(bash.contains("PATH="));
    }
}
