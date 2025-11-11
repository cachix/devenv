//! Arguments passed to the devenv flake template
//!
//! This module defines the structure for arguments passed to the flake template
//! when assembling the devenv environment. The struct is serialized to Nix syntax
//! using the `ser_nix` crate and inserted into the flake template.

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

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

    /// The system string (e.g., "x86_64-linux", "aarch64-darwin")
    pub system: &'a str,

    /// Absolute path to the project root (location of devenv.nix)
    /// Serialized as a Nix path literal by ser_nix
    pub devenv_root: &'a Path,

    /// Project input reference as string
    pub project_input_ref: &'a str,

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

    /// Latest direnvrc version number available
    pub devenv_direnvrc_latest_version: u8,

    /// Container name if building/running a container, otherwise null
    pub container_name: Option<&'a str>,

    /// List of active profiles to enable
    pub active_profiles: &'a [String],

    /// Current system hostname
    pub hostname: Option<&'a str>,

    /// Current username
    pub username: Option<&'a str>,

    /// Git repository root path, if detected; otherwise null
    pub git_root: Option<&'a Path>,

    /// SecretSpec resolved data (profile, provider, secrets)
    pub secretspec: Option<&'a SecretspecData>,
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

        let project_input_ref = format!("path:{}", root.display());
        let args = NixArgs {
            version,
            system,
            devenv_root: &root,
            project_input_ref: &project_input_ref,
            devenv_dotfile: &dotfile,
            devenv_dotfile_path: &dotfile_path,
            devenv_tmpdir: &tmpdir,
            devenv_runtime: &runtime,
            devenv_istesting: false,
            devenv_direnvrc_latest_version: 5,
            container_name,
            active_profiles: &profiles,
            hostname: None,            // None value
            username,                  // Some value
            git_root: Some(&git_root), // Some value with Path type
            secretspec: None,          // None value
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
            serialized.contains(&format!("project_input_ref = \"{}\"", project_input_ref)),
            "project_input_ref key-value pair not found"
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
    }
}
