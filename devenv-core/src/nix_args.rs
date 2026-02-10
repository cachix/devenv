//! Arguments passed to the devenv flake template
//!
//! This module defines the structure for arguments passed to the flake template
//! when assembling the devenv environment. The struct is serialized to Nix syntax
//! using the `ser_nix` crate and inserted into the flake template.

use crate::config::{Config, NixpkgsConfig, SandboxConfig};
use miette::{Result, miette};
use ser_nix::NixLiteral;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use std::collections::HashMap;
use std::path::Path;

/// A parsed CLI option with path and typed value
#[derive(Debug, Clone)]
pub struct CliOption {
    /// The attribute path as a list (e.g., ["languages", "rust", "enable"])
    pub path: Vec<String>,
    /// The typed value
    pub value: CliValue,
}

/// A CLI option value - always serialized as lib.mkForce <value>
/// All values use NixLiteral to ensure proper Nix syntax with lib.mkForce wrapper
#[derive(Debug, Clone)]
pub enum CliValue {
    /// lib.mkForce "string"
    String(String),
    /// lib.mkForce 42
    Int(i64),
    /// lib.mkForce 3.14
    Float(f64),
    /// lib.mkForce true/false
    Bool(bool),
    /// lib.mkForce ./path
    Path(String),
    /// lib.mkForce pkgs.hello
    Pkg(String),
    /// lib.mkForce [ pkgs.hello pkgs.cowsay ]
    PkgList(Vec<String>),
}

impl Serialize for CliValue {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let literal = match self {
            CliValue::String(s) => {
                // Escape quotes in the string
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                format!("lib.mkForce \"{}\"", escaped)
            }
            CliValue::Int(n) => format!("lib.mkForce {}", n),
            CliValue::Float(f) => format!("lib.mkForce {}", f),
            CliValue::Bool(b) => format!("lib.mkForce {}", b),
            CliValue::Path(p) => {
                if p.starts_with('/') || p.starts_with("./") || p.starts_with("../") {
                    format!("lib.mkForce {}", p)
                } else {
                    format!("lib.mkForce ./{}", p)
                }
            }
            CliValue::Pkg(name) => format!("lib.mkForce pkgs.{}", name),
            CliValue::PkgList(names) => {
                let pkgs: Vec<String> = names.iter().map(|n| format!("pkgs.{}", n)).collect();
                format!("lib.mkForce [ {} ]", pkgs.join(" "))
            }
        };
        NixLiteral::from(literal).serialize(serializer)
    }
}

/// CLI options as a nested attrset that serializes directly to Nix syntax
///
/// Converts a list of CLI options like:
///   [{ path: ["languages", "rust", "enable"], value: true }]
/// Into a nested Nix attrset:
///   { languages = { rust = { enable = true; }; }; }
#[derive(Debug, Clone, Default)]
pub struct CliOptionsConfig(pub Vec<CliOption>);

/// Recursive structure to build nested attrsets
#[derive(Debug, Clone)]
enum NestedValue {
    Leaf(CliValue),
    Map(HashMap<String, NestedValue>),
}

impl CliOptionsConfig {
    /// Build nested structure from flat list of options
    fn build_nested(&self) -> NestedValue {
        let mut root: HashMap<String, NestedValue> = HashMap::new();

        for opt in &self.0 {
            Self::insert_at_path(&mut root, &opt.path, &opt.value);
        }

        NestedValue::Map(root)
    }

    /// Insert a value at a nested path
    fn insert_at_path(map: &mut HashMap<String, NestedValue>, path: &[String], value: &CliValue) {
        if path.is_empty() {
            return;
        }

        let key = &path[0];

        if path.len() == 1 {
            // Leaf node - insert the value
            map.insert(key.clone(), NestedValue::Leaf(value.clone()));
        } else {
            // Intermediate node - ensure it's a map and recurse
            let entry = map
                .entry(key.clone())
                .or_insert_with(|| NestedValue::Map(HashMap::new()));

            if let NestedValue::Map(inner_map) = entry {
                Self::insert_at_path(inner_map, &path[1..], value);
            }
        }
    }
}

impl Serialize for NestedValue {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            NestedValue::Leaf(value) => value.serialize(serializer),
            NestedValue::Map(map) => {
                let mut s = serializer.serialize_map(Some(map.len()))?;
                // Sort keys for deterministic output
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                for key in keys {
                    s.serialize_entry(key, &map[key])?;
                }
                s.end()
            }
        }
    }
}

impl Serialize for CliOptionsConfig {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.is_empty() {
            // Empty config - serialize as empty attrset
            let s = serializer.serialize_map(Some(0))?;
            return s.end();
        }

        // Non-empty: serialize as a complete NixOS module function
        // { pkgs, lib, ... }: { config = { ... }; }
        let nested = self.build_nested();

        // Build the inner config attrset as a string
        let inner = ser_nix::to_string(&nested).map_err(serde::ser::Error::custom)?;

        // Wrap as a complete NixOS module function
        let func = format!("{{ pkgs, lib, ... }}: {{ config = {}; }}", inner);
        NixLiteral::from(func).serialize(serializer)
    }
}

/// Parse raw CLI options into structured CliOption values
///
/// Input format: ["key:type", "value", "key2:type2", "value2", ...]
/// Supported types: string, int, float, bool, path, pkg, pkgs
pub fn parse_cli_options(raw_options: &[String]) -> Result<Vec<CliOption>> {
    let mut options = Vec::new();

    // Process pairs of [key:type, value]
    for chunk in raw_options.chunks(2) {
        if chunk.len() != 2 {
            return Err(miette!(
                "CLI options must be provided in pairs (key:type value)"
            ));
        }

        let key_with_type = &chunk[0];
        let value_str = &chunk[1];

        // Split key:type
        let parts: Vec<&str> = key_with_type.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(miette!(
                "CLI option '{}' must include type (e.g., key:string, key:bool)",
                key_with_type
            ));
        }

        let key = parts[0];
        let type_name = parts[1];

        // Split the key path by dots
        let path: Vec<String> = key.split('.').map(|s| s.to_string()).collect();

        // Parse value based on type
        let value = match type_name {
            "string" => CliValue::String(value_str.clone()),
            "int" => {
                let n: i64 = value_str.parse().map_err(|_| {
                    miette!("Invalid integer value '{}' for option '{}'", value_str, key)
                })?;
                CliValue::Int(n)
            }
            "float" => {
                let f: f64 = value_str.parse().map_err(|_| {
                    miette!("Invalid float value '{}' for option '{}'", value_str, key)
                })?;
                CliValue::Float(f)
            }
            "bool" => {
                let b = match value_str.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err(miette!(
                            "Invalid bool value '{}' for option '{}' (expected 'true' or 'false')",
                            value_str,
                            key
                        ));
                    }
                };
                CliValue::Bool(b)
            }
            "path" => CliValue::Path(value_str.clone()),
            "pkg" => CliValue::Pkg(value_str.clone()),
            "pkgs" => {
                let names: Vec<String> = value_str
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                CliValue::PkgList(names)
            }
            other => {
                return Err(miette!(
                    "Unsupported type '{}' for option '{}'. Supported types: string, int, float, bool, path, pkg, pkgs",
                    other,
                    key
                ));
            }
        };

        options.push(CliOption { path, value });
    }

    Ok(options)
}

/// SecretSpec data containing loaded secrets and metadata
#[derive(Debug, Clone, Serialize)]
pub struct SecretspecData {
    /// The profile that was used to load secrets
    pub profile: String,

    /// The provider that was used to load secrets
    pub provider: String,

    /// Map of secret names to their values
    pub secrets: HashMap<String, String>,
}

/// Arguments passed to Nix when assembling the environment
#[derive(Debug, Clone, Serialize)]
pub struct NixArgs<'a> {
    /// The devenv CLI version (e.g., "1.10.1")
    pub version: &'a str,

    /// Whether this is a development build (not from a release tag)
    pub is_development_version: bool,

    /// The system string (e.g., "x86_64-linux", "aarch64-darwin")
    pub system: &'a str,

    /// Absolute path to the project root (location of devenv.nix)
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_root: &'a Path,

    /// Whether to skip loading the local devenv.nix (used when --from is provided)
    pub skip_local_src: bool,

    /// Absolute path to the devenv dotfile directory (.devenv)
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_dotfile: &'a Path,

    /// Relative Nix path to the dotfile directory (e.g., ".devenv")
    /// Serialized as a Nix path literal by ser_nix
    #[serde(serialize_with = "ser_nix::as_nix_path")]
    pub devenv_dotfile_path: &'a Path,

    /// Absolute path to the system temporary directory
    /// Serialized as a Nix path literal by ser_nix
    /// TODO: remove in the next release
    pub devenv_tmpdir: &'a Path,

    /// Absolute path to the runtime directory for this shell session
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_runtime: &'a Path,

    /// Whether the environment is being assembled for testing
    pub devenv_istesting: bool,

    /// Sandbox configuration
    pub devenv_sandbox: Option<&'a SandboxConfig>,

    /// Latest direnvrc version number available
    pub devenv_direnvrc_latest_version: u8,

    /// Container name if building/running a container, otherwise null
    pub container_name: Option<&'a str>,

    /// List of active profiles to enable
    pub active_profiles: &'a [String],

    /// CLI options passed via -O/--option flag
    /// Serializes directly as a nested Nix attrset
    pub cli_options: CliOptionsConfig,

    /// Current system hostname
    pub hostname: Option<&'a str>,

    /// Current username
    pub username: Option<&'a str>,

    /// Git repository root path, if detected; otherwise null
    pub git_root: Option<&'a Path>,

    /// SecretSpec resolved data (profile, provider, secrets)
    pub secretspec: Option<&'a SecretspecData>,

    /// devenv.yaml configuration (inputs, imports, nixpkgs settings, etc.)
    /// Serialized by ser_nix into a Nix attrset
    pub devenv_config: &'a Config,

    /// Pre-merged nixpkgs configuration for the target system.
    /// This is computed by Config::nixpkgs_config() in Rust to avoid
    /// duplicating the merging logic in Nix (bootstrapLib.nix).
    pub nixpkgs_config: NixpkgsConfig,

    /// Content fingerprint of the lock file computed from all inputs' narHashes.
    /// This is used for eval-cache invalidation when local inputs change.
    /// Unlike the serialized lock file, this includes narHashes for path inputs
    /// which are normally stripped when writing to disk.
    pub lock_fingerprint: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper function to check key-value pairs in Nix serialized output.
    /// Matches pattern: key = value (with possible whitespace variation)
    fn contains_key_value(output: &str, key: &str, value: &str) -> bool {
        output.contains(&format!("{} = {}", key, value))
    }

    #[test]
    fn test_nix_args_serialization() {
        // Test with mixed Some and None optional fields.
        // This test documents behavior when ser_nix is fixed to include null values.

        let version = "1.10.1";
        let system = "aarch64-darwin";
        let root = PathBuf::from("/home/user/project");
        let dotfile = PathBuf::from("/home/user/project/.devenv");
        let dotfile_path = PathBuf::from("./.devenv");
        let tmpdir = PathBuf::from("/tmp");
        let runtime = PathBuf::from("/tmp/runtime");
        let git_root = PathBuf::from("/home/user");
        let container_name = Some("my-container");
        let profiles = vec!["frontend".to_string(), "backend".to_string()];
        let username = Some("testuser");

        let test_config = Config::default();
        let nixpkgs_config = test_config.nixpkgs_config(system);
        let cli_options = CliOptionsConfig::default();
        let lock_fingerprint = "abc123";
        let args = NixArgs {
            version,
            is_development_version: false,
            system,
            devenv_root: &root,
            skip_local_src: false,
            devenv_dotfile: &dotfile,
            devenv_dotfile_path: &dotfile_path,
            devenv_tmpdir: &tmpdir,
            devenv_runtime: &runtime,
            devenv_istesting: false,
            devenv_sandbox: None,
            devenv_direnvrc_latest_version: 5,
            container_name,
            active_profiles: &profiles,
            cli_options,
            hostname: None,            // None value
            username,                  // Some value
            git_root: Some(&git_root), // Some value with Path type
            secretspec: None,          // None value
            devenv_config: &test_config,
            nixpkgs_config,
            lock_fingerprint,
        };

        let serialized = ser_nix::to_string(&args).expect("Failed to serialize NixArgs");

        // Verify required fields with correct values
        assert!(
            contains_key_value(&serialized, "version", "\"1.10.1\""),
            "version key-value pair not found"
        );
        assert!(
            contains_key_value(&serialized, "system", "\"aarch64-darwin\""),
            "system key-value pair not found"
        );
        assert!(
            contains_key_value(&serialized, "skip_local_src", "false"),
            "skip_local_src key-value pair not found"
        );
        assert!(
            contains_key_value(&serialized, "devenv_istesting", "false"),
            "devenv_istesting key-value pair not found"
        );
        assert!(
            contains_key_value(&serialized, "devenv_direnvrc_latest_version", "5"),
            "devenv_direnvrc_latest_version key-value pair not found"
        );

        // Verify Some optional fields are present
        assert!(
            contains_key_value(&serialized, "container_name", "\"my-container\""),
            "container_name (Some) key-value pair not found"
        );
        assert!(
            contains_key_value(&serialized, "username", "\"testuser\""),
            "username (Some) key-value pair not found"
        );
        assert!(
            contains_key_value(&serialized, "git_root", "\"/home/user\""),
            "git_root (Some) with Path type key-value pair not found"
        );

        assert!(
            contains_key_value(&serialized, "hostname", "null"),
            "hostname (None) should serialize as null"
        );

        // Verify active_profiles is a Nix list with expected values
        let expected_profiles = "[\n    \"frontend\"\n    \"backend\"\n  ]";
        assert!(
            contains_key_value(&serialized, "active_profiles", expected_profiles),
            "active_profiles list with values not found"
        );

        // Verify cli_options is serialized as empty attrset
        let expected_cli_options = "{\n  }";
        assert!(
            contains_key_value(&serialized, "cli_options", expected_cli_options),
            "cli_options should serialize as empty attrset. Got:\n{}",
            serialized
        );
    }

    #[test]
    fn test_cli_options_serialization_with_values() {
        let version = "1.10.1";
        let system = "x86_64-linux";
        let root = PathBuf::from("/tmp/test");
        let dotfile = PathBuf::from("/tmp/test/.devenv");
        let dotfile_path = PathBuf::from("./.devenv");
        let tmpdir = PathBuf::from("/tmp");
        let runtime = PathBuf::from("/tmp/runtime");
        let profiles: Vec<String> = vec![];

        let test_config = Config::default();
        let nixpkgs_config = test_config.nixpkgs_config(system);

        // Test with parsed CLI options (using parse_cli_options helper)
        let raw_options = vec![
            "languages.python.enable:bool".to_string(),
            "true".to_string(),
        ];
        let cli_options =
            CliOptionsConfig(parse_cli_options(&raw_options).expect("Failed to parse CLI options"));

        let args = NixArgs {
            version,
            is_development_version: false,
            system,
            devenv_root: &root,
            skip_local_src: false,
            devenv_dotfile: &dotfile,
            devenv_dotfile_path: &dotfile_path,
            devenv_tmpdir: &tmpdir,
            devenv_runtime: &runtime,
            devenv_istesting: false,
            devenv_sandbox: None,
            devenv_direnvrc_latest_version: 5,
            container_name: None,
            active_profiles: &profiles,
            cli_options,
            hostname: None,
            username: None,
            git_root: None,
            secretspec: None,
            devenv_config: &test_config,
            nixpkgs_config,
            lock_fingerprint: "",
        };

        let serialized = ser_nix::to_string(&args).expect("Failed to serialize NixArgs");

        // Verify cli_options serializes as a NixOS module function:
        // { pkgs, lib, ... }: { config = { languages = { python = { enable = lib.mkForce true; }; }; }; }
        assert!(
            serialized.contains("cli_options = { pkgs, lib, ... }:"),
            "cli_options should be a NixOS module function. Got:\n{}",
            serialized
        );
        assert!(
            serialized.contains("config = {"),
            "cli_options should have 'config' wrapper. Got:\n{}",
            serialized
        );
        assert!(
            serialized.contains("languages = {"),
            "cli_options should have 'languages' key. Got:\n{}",
            serialized
        );
        assert!(
            serialized.contains("python = {"),
            "cli_options.languages should have 'python' key. Got:\n{}",
            serialized
        );
        assert!(
            serialized.contains("enable = lib.mkForce true"),
            "cli_options.languages.python.enable should be lib.mkForce true. Got:\n{}",
            serialized
        );
    }

    #[test]
    fn test_parse_cli_options() {
        // Test basic types
        let raw = vec![
            "name:string".to_string(),
            "test".to_string(),
            "count:int".to_string(),
            "42".to_string(),
            "enabled:bool".to_string(),
            "true".to_string(),
        ];
        let options = parse_cli_options(&raw).expect("Failed to parse options");
        assert_eq!(options.len(), 3);

        // First option: string
        assert_eq!(options[0].path, vec!["name"]);
        assert!(matches!(options[0].value, CliValue::String(ref s) if s == "test"));

        // Second option: int
        assert_eq!(options[1].path, vec!["count"]);
        assert!(matches!(options[1].value, CliValue::Int(42)));

        // Third option: bool
        assert_eq!(options[2].path, vec!["enabled"]);
        assert!(matches!(options[2].value, CliValue::Bool(true)));
    }

    #[test]
    fn test_parse_cli_options_nested_path() {
        let raw = vec!["languages.rust.enable:bool".to_string(), "true".to_string()];
        let options = parse_cli_options(&raw).expect("Failed to parse options");
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].path, vec!["languages", "rust", "enable"]);
    }

    #[test]
    fn test_cli_value_path_serialization() {
        let abs = CliValue::Path("/abs/path".to_string());
        let rel = CliValue::Path("relative/path".to_string());
        let already = CliValue::Path("./already".to_string());
        let parent = CliValue::Path("../parent".to_string());

        let abs_ser = ser_nix::to_string(&abs).expect("Failed to serialize abs path");
        let rel_ser = ser_nix::to_string(&rel).expect("Failed to serialize rel path");
        let already_ser = ser_nix::to_string(&already).expect("Failed to serialize ./ path");
        let parent_ser = ser_nix::to_string(&parent).expect("Failed to serialize ../ path");

        assert!(
            abs_ser.contains("lib.mkForce /abs/path"),
            "absolute paths should serialize without ./ prefix: {abs_ser}"
        );
        assert!(
            rel_ser.contains("lib.mkForce ./relative/path"),
            "relative paths should be prefixed with ./: {rel_ser}"
        );
        assert!(
            already_ser.contains("lib.mkForce ./already"),
            "already-relative paths should be preserved: {already_ser}"
        );
        assert!(
            parent_ser.contains("lib.mkForce ../parent"),
            "parent-relative paths should be preserved: {parent_ser}"
        );
    }

    #[test]
    fn test_parse_cli_options_pkg() {
        let raw = vec!["mypackage:pkg".to_string(), "hello".to_string()];
        let options = parse_cli_options(&raw).expect("Failed to parse options");
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].path, vec!["mypackage"]);

        // Verify it serializes as lib.mkForce pkgs.hello
        let serialized = ser_nix::to_string(&options[0].value).expect("Failed to serialize");
        assert_eq!(serialized, "lib.mkForce pkgs.hello");
    }

    #[test]
    fn test_parse_cli_options_pkgs_list() {
        let raw = vec!["packages:pkgs".to_string(), "hello cowsay".to_string()];
        let options = parse_cli_options(&raw).expect("Failed to parse options");
        assert_eq!(options.len(), 1);

        // Verify it's a PkgList
        assert!(matches!(options[0].value, CliValue::PkgList(_)));

        // Verify serialization
        let serialized = ser_nix::to_string(&options[0].value).expect("Failed to serialize");
        assert_eq!(serialized, "lib.mkForce [ pkgs.hello pkgs.cowsay ]");
    }

    #[test]
    fn test_cli_options_config_nested_serialization() {
        // Test that CliOptionsConfig serializes as NixOS module function with lib.mkForce
        let raw = vec![
            "languages.rust.channel:string".to_string(),
            "beta".to_string(),
            "services.redis.enable:bool".to_string(),
            "true".to_string(),
        ];
        let options = CliOptionsConfig(parse_cli_options(&raw).expect("Failed to parse options"));
        let serialized = ser_nix::to_string(&options).expect("Failed to serialize");

        // cli_options is now a complete NixOS module function
        let expected = r#"{ pkgs, lib, ... }: { config = {
  languages = {
    rust = {
      channel = lib.mkForce "beta";
    };
  };
  services = {
    redis = {
      enable = lib.mkForce true;
    };
  };
}; }"#;
        assert_eq!(serialized, expected);
    }
}
