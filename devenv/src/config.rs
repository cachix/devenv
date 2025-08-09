use miette::{bail, IntoDiagnostic, Result, WrapErr};
use schemars::{schema_for, JsonSchema};
use schematic::ConfigLoader;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

const YAML_CONFIG: &str = "devenv.yaml";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, schematic::Schematic)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum NixBackendType {
    #[default]
    Nix,
    #[cfg(feature = "snix")]
    Snix,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[config(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct NixpkgsConfig {
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub allow_unfree: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub allow_broken: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub cuda_support: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(merge = schematic::merge::append_vec)]
    pub cuda_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(merge = schematic::merge::append_vec)]
    pub permitted_insecure_packages: Vec<String>,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[config(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct Input {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "is_true", default = "true_default")]
    #[setting(default = true)]
    pub flake: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub follows: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub inputs: BTreeMap<String, Input>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub overlays: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct FlakeInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub follows: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub inputs: BTreeMap<String, Input>,
    #[serde(skip_serializing_if = "is_true", default = "true_default")]
    pub flake: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FlakeInputError {
    UrlAndFollowsBothSet,
}

impl fmt::Display for FlakeInputError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FlakeInputError::UrlAndFollowsBothSet => {
                write!(f, "url and follows cannot both be set for the same input")
            }
        }
    }
}

impl TryFrom<&Input> for FlakeInput {
    type Error = FlakeInputError;

    fn try_from(input: &Input) -> Result<Self, Self::Error> {
        if input.url.is_some() && input.follows.is_some() {
            return Err(Self::Error::UrlAndFollowsBothSet);
        }

        Ok(FlakeInput {
            url: input.url.clone(),
            follows: input.follows.clone(),
            inputs: input.inputs.clone(),
            flake: input.flake,
        })
    }
}

fn true_default() -> bool {
    true
}
#[allow(dead_code)]
fn false_default() -> bool {
    false
}
fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Clean {
    pub enabled: bool,
    pub keep: Vec<String>,
    // TODO: executables?
}

#[derive(schematic::Config, Clone, Serialize, Debug, JsonSchema)]
#[config(rename_all = "camelCase", allow_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Nixpkgs {
    #[serde(flatten)]
    #[setting(nested)]
    pub config_: NixpkgsConfig,
    #[serde(
        rename = "per-platform",
        skip_serializing_if = "BTreeMap::is_empty",
        default
    )]
    #[setting(merge = schematic::merge::merge_btreemap)]
    pub per_platform: BTreeMap<String, NixpkgsConfig>,
}

#[derive(schematic::Config, Clone, Serialize, Debug, JsonSchema)]
#[config(rename_all = "camelCase", allow_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    #[setting(nested, merge = schematic::merge::merge_btreemap)]
    pub inputs: BTreeMap<String, Input>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub allow_unfree: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub allow_broken: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(nested)]
    pub nixpkgs: Option<Nixpkgs>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(merge = schematic::merge::append_vec)]
    pub imports: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(merge = schematic::merge::append_vec)]
    pub permitted_insecure_packages: Vec<String>,
    #[setting(nested)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub clean: Option<Clean>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub impure: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    #[setting(merge = schematic::merge::replace)]
    pub backend: NixBackendType,
    #[setting(nested)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub secretspec: Option<SecretspecConfig>,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SecretspecConfig {
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(default = false)]
    pub enable: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider: Option<String>,
}

// TODO: https://github.com/moonrepo/schematic/issues/105
pub async fn write_json_schema() -> Result<()> {
    let schema = schema_for!(Config);
    let schema = serde_json::to_string_pretty(&schema)
        .into_diagnostic()
        .wrap_err("Failed to serialize JSON schema")?;
    let path = Path::new("docs/devenv.schema.json");
    tokio::fs::write(path, &schema)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write json schema to {}", path.display()))?;
    Ok(())
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from("./")
    }

    /// Loads configuration from a directory path, including all imported configurations.
    ///
    /// This method recursively loads the base `devenv.yaml` file and all configurations
    /// referenced in the `imports` field. Configurations are merged according to their
    /// field-specific merge strategies.
    ///
    /// # Arguments
    /// * `path` - The directory containing the base `devenv.yaml` file
    ///
    /// # Returns
    /// The loaded and merged configuration
    ///
    /// # Errors
    /// Returns an error if:
    /// - A configuration file cannot be parsed
    /// - An import path cannot be resolved
    /// - Circular imports are detected (handled automatically)
    /// - Import depth exceeds the maximum limit
    pub fn load_from<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let base_path = path.as_ref();
        let base_yaml = base_path.join(YAML_CONFIG);

        // Collect all yaml files to load (base + imports)
        let mut yaml_files = Vec::new();
        let mut visited = HashSet::new();

        if base_yaml.exists() {
            let canonical_base =
                base_yaml
                    .canonicalize()
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!("Failed to canonicalize base path: {}", base_yaml.display())
                    })?;
            yaml_files.push(base_yaml.clone());
            visited.insert(canonical_base);
        }

        // Load the base config first to get the imports
        let mut temp_loader = ConfigLoader::<Config>::new();
        let _ = temp_loader.file_optional(&base_yaml);
        let temp_result = temp_loader.load().into_diagnostic().wrap_err_with(|| {
            format!(
                "Failed to load base configuration from: {}",
                base_yaml.display()
            )
        })?;

        // Recursively collect all imported yaml files
        Self::collect_import_files(
            &temp_result.config.imports,
            base_path,
            &mut yaml_files,
            &mut visited,
            0,
        )?;

        // Now load all files in order using a single ConfigLoader
        let mut loader = ConfigLoader::<Config>::new();
        for yaml_file in &yaml_files {
            let _ = loader.file_optional(yaml_file);
        }

        let result = loader
            .load()
            .into_diagnostic()
            .wrap_err("Failed to load and merge all configuration files")?;
        Ok(result.config)
    }

    /// Recursively collects all import files starting from the given imports list.
    ///
    /// This method traverses the import graph depth-first, collecting all `devenv.yaml`
    /// files that need to be loaded. It handles circular imports by tracking visited
    /// files and enforces a maximum recursion depth.
    ///
    /// # Arguments
    /// * `imports` - List of import paths (directories) to process
    /// * `base_path` - The base directory for resolving relative import paths
    /// * `yaml_files` - Accumulator for collecting YAML file paths in load order
    /// * `visited` - Set of canonical paths already visited (prevents circular imports)
    /// * `depth` - Current recursion depth (prevents stack overflow)
    ///
    /// # Returns
    /// Ok(()) if all imports were successfully collected
    ///
    /// # Errors
    /// Returns an error if:
    /// - Maximum import depth is exceeded
    /// - Path traversal is detected (import escapes project directory)
    /// - Import file cannot be read or parsed
    /// - Import path cannot be canonicalized
    fn collect_import_files(
        imports: &[String],
        base_path: &Path,
        yaml_files: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        depth: usize,
    ) -> Result<()> {
        const MAX_IMPORT_DEPTH: usize = 100;

        if depth > MAX_IMPORT_DEPTH {
            bail!(
                "Maximum import depth ({}) exceeded. Check for excessively nested imports.",
                MAX_IMPORT_DEPTH
            );
        }

        for import in imports {
            // Security check: forbid parent directory references
            if import.contains("../") {
                bail!(
                    "Import path '{}' contains '../' which is not allowed. Imports must use relative paths (relative to the current file) or absolute paths (relative to the git root).",
                    import
                );
            }

            let import_path = base_path.join(import);

            if import_path.is_dir() {
                let yaml_path = import_path.join(YAML_CONFIG);

                if yaml_path.exists() {
                    let canonical_path =
                        yaml_path
                            .canonicalize()
                            .into_diagnostic()
                            .wrap_err_with(|| {
                                format!(
                                    "Failed to canonicalize import path: {}",
                                    yaml_path.display()
                                )
                            })?;

                    // Skip if already visited (circular import prevention)
                    if visited.contains(&canonical_path) {
                        continue;
                    }
                    visited.insert(canonical_path.clone());
                    yaml_files.push(yaml_path.clone());

                    // Load this config to get its imports
                    let mut temp_loader = ConfigLoader::<Config>::new();
                    let _ = temp_loader.file_optional(&yaml_path);
                    let temp_result = temp_loader.load().into_diagnostic().wrap_err_with(|| {
                        format!("Failed to load import file: {}", yaml_path.display())
                    })?;

                    // Recursively collect imports from this config
                    // Note: base path is now the directory containing the imported devenv.yaml
                    Self::collect_import_files(
                        &temp_result.config.imports,
                        &import_path,
                        yaml_files,
                        visited,
                        depth + 1,
                    )?;
                }
            }
        }

        Ok(())
    }

    pub async fn write(&self) -> Result<()> {
        let yaml = serde_yaml::to_string(&self)
            .into_diagnostic()
            .wrap_err("Failed to serialize config to YAML")?;
        tokio::fs::write(YAML_CONFIG, yaml)
            .await
            .into_diagnostic()
            .wrap_err("Failed to write devenv.yaml")?;
        Ok(())
    }

    /// Add a new input, overwriting any existing input with the same name.
    pub fn add_input(&mut self, name: &str, url: &str, follows: &[String]) -> Result<()> {
        // A set of inputs built from the follows list.
        let mut inputs = BTreeMap::new();

        // Resolve the follows to top-level inputs.
        // We assume that nixpkgs is always available.
        for follow in follows {
            if self.inputs.contains_key(follow) || follow == "nixpkgs" {
                let input = Input {
                    follows: Some(follow.to_string()),
                    ..Default::default()
                };
                inputs.insert(follow.to_string(), input);
            } else {
                return Err(miette::miette!(
                    "Input {follow} does not exist so it can't be followed."
                ));
            }
        }

        let input = Input {
            url: Some(url.to_string()),
            inputs,
            ..Default::default()
        };
        self.inputs.insert(name.to_string(), input);
        Ok(())
    }

    /// Override the URL of an existing input.
    pub fn override_input_url(&mut self, name: &str, url: &str) -> Result<()> {
        if let Some(input) = self.inputs.get_mut(name) {
            input.url = Some(url.to_string());
            Ok(())
        } else if name == "nixpkgs" || name == "devenv" {
            self.add_input(name, url, &[])
        } else {
            Err(miette::miette!(
                "Input {name} does not exist so it can't be overridden."
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_flake_input_from_input_with_url_and_follows() {
        let input = Input {
            url: Some("github:NixOS/nixpkgs".to_string()),
            follows: Some("nixpkgs".to_string()),
            ..Default::default()
        };
        let result = FlakeInput::try_from(&input);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), FlakeInputError::UrlAndFollowsBothSet);
    }

    #[test]
    fn add_input() {
        let mut config = Config::default();
        config
            .add_input("nixpkgs", "github:NixOS/nixpkgs/nixpkgs-unstable", &[])
            .expect("Failed to add input");
        assert_eq!(config.inputs.len(), 1);
        assert_eq!(
            config.inputs["nixpkgs"].url,
            Some("github:NixOS/nixpkgs/nixpkgs-unstable".to_string())
        );
        assert!(config.inputs["nixpkgs"].flake);
    }

    #[test]
    fn add_input_with_follows() {
        let mut config = Config::default();
        config
            .add_input("other", "github:org/repo", &[])
            .expect("Failed to add input");
        config
            .add_input(
                "input-with-follows",
                "github:org/repo",
                &["nixpkgs".to_string(), "other".to_string()],
            )
            .expect("Failed to add input with follows");
        assert_eq!(config.inputs.len(), 2);
        let input = &config.inputs["input-with-follows"];
        assert_eq!(input.inputs.len(), 2);
    }

    #[test]
    #[should_panic(expected = "Input other does not exist so it can't be followed.")]
    fn add_input_with_missing_follows() {
        let mut config = Config::default();
        let result = config.add_input(
            "input-with-follows",
            "github:org/repo",
            &["other".to_string()],
        );
        result.unwrap(); // This will panic with the Err from add_input
    }

    #[test]
    fn override_input_url() {
        let mut config = Config::default();
        config
            .add_input("nixpkgs", "github:NixOS/nixpkgs/nixpkgs-unstable", &[])
            .expect("Failed to add input");
        assert_eq!(
            config.inputs["nixpkgs"].url,
            Some("github:NixOS/nixpkgs/nixpkgs-unstable".to_string())
        );
        config
            .override_input_url("nixpkgs", "github:NixOS/nixpkgs/nixos-24.11")
            .expect("Failed to override input URL");
        assert_eq!(
            config.inputs["nixpkgs"].url,
            Some("github:NixOS/nixpkgs/nixos-24.11".to_string())
        );
    }

    #[test]
    fn preserve_options_on_override_input_url() {
        let mut config = Config {
            inputs: BTreeMap::from_iter(vec![(
                "non-flake".to_string(),
                Input {
                    url: Some("path:some-path".to_string()),
                    flake: false,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        config
            .override_input_url("non-flake", "path:some-other-path")
            .expect("Failed to override input URL");
        assert!(!config.inputs["non-flake"].flake);
        assert_eq!(
            config.inputs["non-flake"].url,
            Some("path:some-other-path".to_string())
        );
    }

    #[test]
    fn default_config_serializes_to_empty_yaml() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).expect("Failed to serialize config");
        assert_eq!(
            yaml.trim(),
            "{}",
            "Default config should serialize to empty YAML"
        );
    }

    #[test]
    fn test_devenv_yaml_import_merging() {
        let fixture_path = Path::new("fixtures/config/import-merging");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        assert!(config.allow_unfree);
        assert!(config.allow_broken);
        assert!(config.impure);
        assert_eq!(config.inputs.len(), 2);
        assert_eq!(
            config.inputs["nixpkgs"].url,
            Some("github:NixOS/nixpkgs/nixos-23.11".to_string())
        );
        assert_eq!(
            config.inputs["flake-utils"].url,
            Some("github:numtide/flake-utils".to_string())
        );
    }

    #[test]
    fn test_relative_import_paths() {
        let fixture_path = Path::new("fixtures/config/relative-imports");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        assert!(config.allow_unfree);
        assert!(config.allow_broken);
    }

    #[test]
    fn test_circular_import_prevention() {
        let fixture_path = Path::new("fixtures/config/circular-imports");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        assert!(config.allow_unfree);
        assert!(config.allow_broken);
    }

    #[test]
    fn test_nixpkgs_config_merging() {
        let fixture_path = Path::new("fixtures/config/nixpkgs-merging");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        let nixpkgs = config.nixpkgs.expect("nixpkgs should be present");
        assert!(nixpkgs.config_.allow_unfree);
        assert!(nixpkgs.config_.allow_broken);
        assert!(nixpkgs.config_.cuda_support);
        assert_eq!(nixpkgs.config_.cuda_capabilities.len(), 3);
        assert!(nixpkgs
            .config_
            .cuda_capabilities
            .contains(&"7.5".to_string()));
        assert!(nixpkgs
            .config_
            .cuda_capabilities
            .contains(&"8.0".to_string()));
        assert!(nixpkgs
            .config_
            .cuda_capabilities
            .contains(&"8.6".to_string()));
        assert_eq!(nixpkgs.config_.permitted_insecure_packages.len(), 1);
    }

    #[test]
    fn test_nested_imports() {
        let fixture_path = Path::new("fixtures/config/nested-imports");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        assert!(config.allow_unfree);
        assert!(config.allow_broken);
        assert!(config.impure);
        assert_eq!(config.inputs.len(), 1);
        assert_eq!(
            config.inputs["custom"].url,
            Some("github:user/repo".to_string())
        );
    }

    #[test]
    fn test_duplicate_imports() {
        let fixture_path = Path::new("fixtures/config/duplicate-imports");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        // Test that shared is imported multiple times but allowUnfree is still true
        assert!(config.allow_unfree);

        // Test array appending with duplicates
        // Should contain all entries including duplicates from append_vec merge strategy
        assert!(config
            .permitted_insecure_packages
            .contains(&"openssl-1.0.2".to_string()));
        assert!(config
            .permitted_insecure_packages
            .contains(&"shared-package".to_string()));
        assert!(config
            .permitted_insecure_packages
            .contains(&"module1-package".to_string()));
        assert!(config
            .permitted_insecure_packages
            .contains(&"module2-package".to_string()));

        // Check that duplicates are preserved (append_vec doesn't deduplicate)
        let openssl_count = config
            .permitted_insecure_packages
            .iter()
            .filter(|&p| p == "openssl-1.0.2")
            .count();
        assert!(
            openssl_count >= 2,
            "Expected duplicate openssl entries, got {}",
            openssl_count
        );
    }

    #[test]
    fn test_boolean_merging_last_wins() {
        let fixture_path = Path::new("fixtures/config/boolean-merging");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        // With replace strategy, last import wins
        // Order: base (false) -> first (true) -> second (false) -> third (true for allowUnfree only)
        assert!(config.allow_unfree); // third sets this to true
        assert!(!config.allow_broken); // second set to false, third doesn't change
        assert!(!config.impure); // second set to false, third doesn't change
    }

    #[test]
    fn test_array_duplicates_behavior() {
        let fixture_path = Path::new("fixtures/config/array-duplicates");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        let nixpkgs = config.nixpkgs.expect("nixpkgs should be present");

        // With append_vec, all entries are preserved including duplicates
        // Base: ["7.5", "7.5", "8.0"] + sub1: ["8.0", "8.6", "8.0"] + sub2: ["7.5", "9.0"]
        let capabilities = &nixpkgs.config_.cuda_capabilities;
        assert!(capabilities.contains(&"7.5".to_string()));
        assert!(capabilities.contains(&"8.0".to_string()));
        assert!(capabilities.contains(&"8.6".to_string()));
        assert!(capabilities.contains(&"9.0".to_string()));

        // Count duplicates
        let count_7_5 = capabilities.iter().filter(|&c| c == "7.5").count();
        let count_8_0 = capabilities.iter().filter(|&c| c == "8.0").count();
        assert!(
            count_7_5 >= 3,
            "Expected at least 3 occurrences of 7.5, got {}",
            count_7_5
        );
        assert!(
            count_8_0 >= 3,
            "Expected at least 3 occurrences of 8.0, got {}",
            count_8_0
        );
    }

    #[test]
    fn test_complex_nixpkgs_merging() {
        let fixture_path = Path::new("fixtures/config/complex-nixpkgs");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        let nixpkgs = config.nixpkgs.expect("nixpkgs should be present");

        // Top-level nixpkgs config
        assert!(nixpkgs.config_.allow_unfree); // from base
        assert!(nixpkgs.config_.allow_broken); // from platform-specific
        assert!(nixpkgs.config_.cuda_support); // from cuda-config

        // Per-platform configs (merge_btreemap replaces entire entries)
        let x86_linux = &nixpkgs.per_platform["x86_64-linux"];
        // The entire x86_64-linux entry is replaced by cuda-config which only has cudaCapabilities
        assert!(!x86_linux.cuda_support); // default false, as cuda-config replaces the whole entry
        assert_eq!(x86_linux.cuda_capabilities, vec!["9.0"]); // from cuda-config (last wins)

        let x86_darwin = &nixpkgs.per_platform["x86_64-darwin"];
        assert!(x86_darwin.allow_unfree); // from platform-specific

        let aarch64_darwin = &nixpkgs.per_platform["aarch64-darwin"];
        assert!(aarch64_darwin.allow_broken); // from platform-specific
        assert!(!aarch64_darwin.cuda_support); // default false (not merged from base)
    }

    #[test]
    fn test_path_edge_cases() {
        let fixture_path = Path::new("fixtures/config/path-edge-cases");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        // Should load normal and "with-spaces in name" but silently ignore non-existent
        assert!(config.allow_unfree); // from normal
        assert!(config.allow_broken); // from "with-spaces in name"
    }

    #[test]
    fn test_empty_configs() {
        let fixture_path = Path::new("fixtures/config/empty-configs");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        // Empty base config with imports should still work
        assert!(config.allow_unfree); // from with-content

        // Other fields should have default values
        assert!(!config.allow_broken);
        assert!(!config.impure);
        assert!(config.imports.len() >= 3); // base imports are preserved
    }

    #[test]
    fn test_import_order_diamond_pattern() {
        let fixture_path = Path::new("fixtures/config/import-order");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        // All imports should be processed despite diamond pattern
        assert!(config.allow_unfree); // from b
        assert!(config.allow_broken); // from c
        assert!(config.impure); // from d (imported by both b and c)

        // Input from d should only appear once despite being imported twice
        assert_eq!(config.inputs.len(), 1);
        assert_eq!(
            config.inputs["shared-input"].url,
            Some("github:shared/repo".to_string())
        );
    }

    #[test]
    fn test_optional_configs_merging() {
        let fixture_path = Path::new("fixtures/config/optional-configs");
        let config = Config::load_from(fixture_path).expect("Failed to load config");

        // Clean config should be overridden (replace strategy for nested Option)
        let clean = config.clean.expect("clean should be present");
        assert!(!clean.enabled); // overridden from true to false
        assert_eq!(clean.keep, vec!["build", "dist"]); // completely replaced

        // Secretspec should be added
        let secretspec = config.secretspec.expect("secretspec should be present");
        assert!(secretspec.enable);
        assert_eq!(secretspec.profile, Some("dev".to_string()));
        assert_eq!(secretspec.provider, Some("aws".to_string()));
    }

    #[test]
    fn test_path_traversal_prevention() {
        let fixture_path = Path::new("fixtures/config/path-traversal");
        let result = Config::load_from(fixture_path);

        assert!(result.is_err(), "Expected error but got: {:?}", result);
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("contains '../' which is not allowed"),
            "Expected path traversal error, got: {}",
            error_message
        );
    }

    #[test]
    fn test_missing_import_file_error() {
        let fixture_path = Path::new("fixtures/config/empty-configs");
        // This test works because empty-configs has imports to non-existent directories
        // which are silently ignored (no devenv.yaml file exists)
        let config = Config::load_from(fixture_path)
            .expect("Missing import files should be silently ignored");
        assert!(config.allow_unfree); // Should still load successfully
    }

    #[test]
    fn test_invalid_yaml_error_context() {
        use std::fs;
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let invalid_yaml_path = temp_dir.path().join("devenv.yaml");

        fs::write(&invalid_yaml_path, "invalid: yaml: content: [").expect("Failed to write file");

        let result = Config::load_from(temp_dir.path());
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("devenv.yaml") || error_message.contains("Failed to load"),
            "Error should mention the file that failed to load: {}",
            error_message
        );
    }
}
