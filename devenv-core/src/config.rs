use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pathdiff;
use schemars::{JsonSchema, schema_for};
use schematic::ConfigLoader;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

const YAML_CONFIG: &str = "devenv.yaml";
const YAML_LOCAL_CONFIG: &str = "devenv.local.yaml";

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
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub permitted_unfree_packages: Vec<String>,
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(merge = schematic::merge::replace)]
    pub profile: Option<String>,
    /// Git repository root path (not serialized, computed during load)
    #[serde(skip)]
    pub git_root: Option<PathBuf>,
    /// Resolved active profiles (not serialized, computed by apply_cli_overrides)
    #[serde(skip)]
    pub profiles: Vec<String>,
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
    let path = Path::new("docs/src/devenv.schema.json");
    tokio::fs::write(path, &schema)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write json schema to {}", path.display()))?;
    Ok(())
}

impl From<&Path> for Config {
    fn from(path: &Path) -> Self {
        Self::load_from(path).expect("Failed to load config with imports")
    }
}

impl From<PathBuf> for Config {
    fn from(path: PathBuf) -> Self {
        Self::from(path.as_path())
    }
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
        temp_loader
            .file_optional(&base_yaml)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("Failed to load configuration file: {}", base_yaml.display())
            })?;
        let temp_result = temp_loader.load().into_diagnostic().wrap_err_with(|| {
            format!(
                "Failed to parse configuration from: {}",
                base_yaml.display()
            )
        })?;

        // Detect git repository root for import resolution
        let git_root = Self::detect_git_root(base_path);

        // Recursively collect all imported yaml files
        Self::collect_import_files(
            &temp_result.config.imports,
            base_path,
            git_root.as_deref(),
            &mut yaml_files,
            &mut visited,
            0,
        )?;

        // Load all configs and track which inputs come from which config file
        // This is needed to correctly normalize relative URLs
        let mut loader = ConfigLoader::<Config>::new();
        let mut input_source_dirs: HashMap<String, PathBuf> = HashMap::new();

        for yaml_file in &yaml_files {
            let config_dir = yaml_file.parent().unwrap_or(Path::new(".")).to_path_buf();

            // Load this config file to see what inputs it defines
            let mut single_loader = ConfigLoader::<Config>::new();
            single_loader
                .file_optional(yaml_file)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("Failed to load configuration file: {}", yaml_file.display())
                })?;
            let single_result = single_loader.load().into_diagnostic().wrap_err_with(|| {
                format!(
                    "Failed to parse configuration from: {}",
                    yaml_file.display()
                )
            })?;

            // Record the source directory for each input defined in this config
            // Earlier configs take precedence (first definition wins)
            for input_name in single_result.config.inputs.keys() {
                input_source_dirs
                    .entry(input_name.clone())
                    .or_insert_with(|| config_dir.clone());
            }

            loader
                .file_optional(yaml_file)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("Failed to load configuration file: {}", yaml_file.display())
                })?;
        }

        // Load devenv.local.yaml last (if it exists) to allow local overrides
        let local_yaml = base_path.join(YAML_LOCAL_CONFIG);
        if local_yaml.exists() {
            // Track inputs from local yaml too
            let mut local_loader = ConfigLoader::<Config>::new();
            local_loader
                .file_optional(&local_yaml)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!(
                        "Failed to load local configuration file: {}",
                        local_yaml.display()
                    )
                })?;
            if let Ok(local_result) = local_loader.load().into_diagnostic() {
                for input_name in local_result.config.inputs.keys() {
                    input_source_dirs
                        .entry(input_name.clone())
                        .or_insert_with(|| base_path.to_path_buf());
                }
            }
        }
        loader
            .file_optional(&local_yaml)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to load local configuration file: {}",
                    local_yaml.display()
                )
            })?;

        let result = loader
            .load()
            .into_diagnostic()
            .wrap_err("Failed to load and merge all configuration files")?;

        let mut config = result.config;

        // Normalize relative URLs in inputs using the tracked source directories
        for (name, input) in config.inputs.iter_mut() {
            if let Some(url) = &input.url {
                let (had_prefix, full_path_str) = if let Some(stripped) = url.strip_prefix("path:")
                {
                    (true, stripped)
                } else if url.starts_with("./") || url.starts_with("../") {
                    (false, url.as_str())
                } else {
                    continue;
                };

                // Separate path from query parameters (e.g., ".?dir=src/modules" -> ".", "?dir=src/modules")
                let (path_str, query_params) = full_path_str
                    .split_once('?')
                    .map_or((full_path_str, ""), |(p, q)| (p, q));

                // Check if this was an absolute path (starts with / after stripping path: prefix)
                // path:///foo and path:/foo are both absolute paths
                let was_absolute = path_str.starts_with('/');

                // Use the tracked source directory for this input, or fall back to base_path
                let source_dir = input_source_dirs
                    .get(name)
                    .map(|p| p.as_path())
                    .unwrap_or(base_path);
                let resolved = source_dir.join(path_str);

                if let Some(rel_to_base) = Self::normalize_path(&resolved, base_path) {
                    let query_suffix = if query_params.is_empty() {
                        String::new()
                    } else {
                        format!("?{}", query_params)
                    };

                    // If the original path was absolute and the result would escape the base
                    // directory (starts with ../), preserve the absolute path instead.
                    // This is necessary for lazy-trees mode in Nix, which can't access
                    // paths outside the flake root via relative paths.
                    let is_outside_base = rel_to_base.starts_with("../");
                    if was_absolute && is_outside_base {
                        // Preserve the absolute path - canonicalize it first
                        if let Ok(canonical) = resolved.canonicalize() {
                            let new_url = format!("path:{}{}", canonical.display(), query_suffix);
                            input.url = Some(new_url);
                        }
                        // If canonicalization fails, leave the URL unchanged
                    } else {
                        let new_url = if had_prefix {
                            let stripped = rel_to_base.strip_prefix("./").unwrap_or(&rel_to_base);
                            // Use "." for current directory when strip_prefix results in empty string
                            let path_part = if stripped.is_empty() { "." } else { stripped };
                            format!("path:{}{}", path_part, query_suffix)
                        } else {
                            format!("{}{}", rel_to_base, query_suffix)
                        };
                        input.url = Some(new_url);
                    }
                }
            }
        }

        // Rebuild imports: normalize file imports we loaded, preserve everything else
        let mut final_imports = Vec::new();
        let mut seen = HashSet::new();

        // Add all loaded file imports (normalized)
        for yaml_path in yaml_files.iter().skip(1) {
            if let Some(import_dir) = yaml_path.parent()
                && let Some(normalized) = Self::normalize_path(import_dir, base_path)
                && seen.insert(normalized.clone())
            {
                final_imports.push(normalized);
            }
        }

        // Add imports from base config that weren't loaded as files
        for import in &temp_result.config.imports {
            let normalized = if import.starts_with('/') {
                // Transform git-root path to relative path
                if let Some(root) = &git_root {
                    let absolute_path = root.join(import.strip_prefix('/').unwrap());
                    Self::normalize_path(&absolute_path, base_path)
                        .unwrap_or_else(|| import.clone())
                } else {
                    import.clone()
                }
            } else {
                import.clone()
            };

            if seen.insert(normalized.clone()) {
                final_imports.push(normalized);
            }
        }

        // Add non-file-based imports from merged config
        for import in &config.imports {
            if !Self::is_file_import(import) && seen.insert(import.clone()) {
                final_imports.push(import.clone());
            }
        }

        config.imports = final_imports;
        config.git_root = git_root;

        Ok(config)
    }

    /// Detects the git repository root starting from the given path.
    ///
    /// # Arguments
    /// * `start_path` - The directory to start searching from
    ///
    /// # Returns
    /// Some(PathBuf) with the git root if found, None otherwise
    fn detect_git_root(start_path: &Path) -> Option<PathBuf> {
        std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(start_path)
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    Some(PathBuf::from(path_str))
                } else {
                    None
                }
            })
    }

    /// Checks if an import is a file-based import (relative or absolute path).
    fn is_file_import(import: &str) -> bool {
        import.starts_with("./") || import.starts_with("../") || import.starts_with('/')
    }

    /// Normalizes a path relative to a base directory.
    /// Returns a path string with ./ prefix for local paths, or no prefix for ../ paths.
    fn normalize_path(source: &Path, base: &Path) -> Option<String> {
        let canonical_source = source.canonicalize().ok();
        let canonical_base = base.canonicalize().ok();

        match (canonical_source, canonical_base) {
            (Some(src), Some(base_canon)) => pathdiff::diff_paths(&src, &base_canon).map(|rel| {
                let rel_str = rel.display().to_string();
                if rel_str.starts_with("../") || rel_str.starts_with("..\\") {
                    rel_str
                } else {
                    format!("./{}", rel_str)
                }
            }),
            _ => {
                // Fallback if canonicalization fails
                pathdiff::diff_paths(source, base).map(|rel| {
                    let rel_str = rel.display().to_string();
                    if rel_str.starts_with("../") || rel_str.starts_with("..\\") {
                        rel_str
                    } else {
                        format!("./{}", rel_str)
                    }
                })
            }
        }
    }

    /// Validates that an import path stays within a security root.
    fn validate_within_root(
        import_path: &Path,
        security_root: &Path,
        import: &str,
        git_root: Option<&Path>,
    ) -> Result<()> {
        let canonical_import = import_path.canonicalize().ok();
        let canonical_root = security_root.canonicalize().ok();

        // Try to validate using canonical paths if both exist
        if let (Some(import_canon), Some(root_canon)) = (&canonical_import, &canonical_root) {
            if !import_canon.starts_with(root_canon) {
                bail!(
                    "Import path '{}' resolves outside the {} which is not allowed. Imports must stay within the {}.",
                    import,
                    if git_root.is_some() {
                        "git repository"
                    } else {
                        "base directory"
                    },
                    if git_root.is_some() {
                        "repository"
                    } else {
                        "project directory"
                    }
                );
            }
        } else if canonical_import.is_none()
            && let Some(canonical_root) = canonical_root
        {
            // Import path doesn't exist, but root does - validate lexically
            // First make the import path absolute, then normalize
            let abs_import = if import_path.is_absolute() {
                Self::normalize_path_components(import_path)
            } else {
                // Make relative path absolute from current directory first
                if let Ok(cwd) = std::env::current_dir() {
                    let absolute = cwd.join(import_path);
                    Self::normalize_path_components(&absolute)
                } else {
                    // Can't get cwd, skip validation
                    return Ok(());
                }
            };

            if !abs_import.starts_with(&canonical_root) {
                bail!(
                    "Import path '{}' resolves outside the {} which is not allowed. Imports must stay within the {}.",
                    import,
                    if git_root.is_some() {
                        "git repository"
                    } else {
                        "base directory"
                    },
                    if git_root.is_some() {
                        "repository"
                    } else {
                        "project directory"
                    }
                );
            }
        }
        // If both paths don't exist or only root doesn't exist, skip validation
        // The path will be validated when it's actually used

        Ok(())
    }

    /// Normalizes a path by resolving `.` and `..` components without requiring the path to exist.
    fn normalize_path_components(path: &Path) -> PathBuf {
        let mut components = Vec::new();

        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    // Pop the last component if it's not a root component
                    if let Some(last) = components.last() {
                        match last {
                            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                                // Don't pop root components
                            }
                            _ => {
                                components.pop();
                            }
                        }
                    }
                }
                std::path::Component::CurDir => {
                    // Skip current directory references
                }
                _ => {
                    components.push(component);
                }
            }
        }

        components.iter().collect()
    }

    /// Resolves an import path relative to base_path or git_root.
    fn resolve_import_path(
        import: &str,
        base_path: &Path,
        git_root: Option<&Path>,
    ) -> Result<PathBuf> {
        let resolved = if import.starts_with('/') {
            if let Some(root) = git_root {
                root.join(import.strip_prefix('/').unwrap())
            } else {
                bail!(
                    "Absolute import path '{}' requires a git repository. Use relative paths (e.g., './' or '../') instead.",
                    import
                );
            }
        } else {
            base_path.join(import)
        };

        // Security check
        let security_root = git_root.unwrap_or(base_path);
        Self::validate_within_root(&resolved, security_root, import, git_root)?;

        Ok(resolved)
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
    /// * `git_root` - Optional git repository root for resolving absolute paths and security checks
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
    /// - Path traversal is detected (import escapes git repository or base directory)
    /// - Import file cannot be read or parsed
    /// - Import path cannot be canonicalized
    fn collect_import_files(
        imports: &[String],
        base_path: &Path,
        git_root: Option<&Path>,
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
            // Resolve and validate the import path
            let import_path = Self::resolve_import_path(import, base_path, git_root)?;

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
                    temp_loader
                        .file_optional(&yaml_path)
                        .into_diagnostic()
                        .wrap_err_with(|| {
                            format!("Failed to load configuration file: {}", yaml_path.display())
                        })?;
                    let temp_result = temp_loader.load().into_diagnostic().wrap_err_with(|| {
                        format!(
                            "Failed to parse configuration from: {}",
                            yaml_path.display()
                        )
                    })?;

                    // Recursively collect imports from this config
                    // Note: base path is now the directory containing the imported devenv.yaml
                    Self::collect_import_files(
                        &temp_result.config.imports,
                        &import_path,
                        git_root,
                        yaml_files,
                        visited,
                        depth + 1,
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Merge CLI overrides into this config so it becomes the single source
    /// of truth for all settings that can be specified in both `devenv.yaml`
    /// and on the command line.
    ///
    /// For `impure`, the merged value is also written back to
    /// `global_options` because the nix backend reads it from there.
    pub fn apply_cli_overrides(&mut self, global_options: &mut crate::cli::GlobalOptions) {
        // impure: either source enables it.
        // Back-propagate to global_options because the nix backend reads from there.
        if global_options.impure {
            self.impure = true;
        } else if self.impure {
            global_options.impure = true;
        }

        // clean: CLI --clean takes precedence over config
        if let Some(ref keep) = global_options.clean {
            self.clean = Some(Clean {
                enabled: true,
                keep: keep.clone(),
            });
        }

        // profiles: CLI --profile takes precedence over config
        self.profiles = if !global_options.profile.is_empty() {
            global_options.profile.clone()
        } else if let Some(ref profile) = self.profile {
            vec![profile.clone()]
        } else {
            Vec::new()
        };

        // secretspec: CLI overrides
        if global_options.secretspec_provider.is_some()
            || global_options.secretspec_profile.is_some()
        {
            let secretspec = self.secretspec.get_or_insert(SecretspecConfig {
                enable: false,
                profile: None,
                provider: None,
            });
            secretspec.enable = true;
            if let Some(ref provider) = global_options.secretspec_provider {
                secretspec.provider = Some(provider.clone());
            }
            if let Some(ref profile) = global_options.secretspec_profile {
                secretspec.profile = Some(profile.clone());
            }
        }
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

    /// Returns the merged nixpkgs configuration for a given system.
    ///
    /// Merges configuration with the following priority (highest to lowest):
    /// 1. `nixpkgs.per_platform.{system}.{field}`
    /// 2. `nixpkgs.{field}` (base nixpkgs config)
    /// 3. Top-level `{field}` (for allow_unfree, allow_broken, permitted_insecure_packages)
    /// 4. Default value
    ///
    /// This matches the logic in bootstrapLib.nix's getPlatformConfig helper.
    pub fn nixpkgs_config(&self, system: &str) -> NixpkgsConfig {
        // Start with defaults
        let mut config = NixpkgsConfig::default();

        // Apply top-level settings (lowest priority for these fields)
        config.allow_unfree = self.allow_unfree;
        config.allow_broken = self.allow_broken;
        config.permitted_insecure_packages = self.permitted_insecure_packages.clone();

        // Apply base nixpkgs config (overrides top-level)
        if let Some(ref nixpkgs) = self.nixpkgs {
            let base = &nixpkgs.config_;
            if base.allow_unfree {
                config.allow_unfree = true;
            }
            if base.allow_broken {
                config.allow_broken = true;
            }
            if base.cuda_support {
                config.cuda_support = true;
            }
            if !base.cuda_capabilities.is_empty() {
                config.cuda_capabilities = base.cuda_capabilities.clone();
            }
            if !base.permitted_insecure_packages.is_empty() {
                config.permitted_insecure_packages = base.permitted_insecure_packages.clone();
            }
            if !base.permitted_unfree_packages.is_empty() {
                config.permitted_unfree_packages = base.permitted_unfree_packages.clone();
            }

            // Apply per-platform config (highest priority)
            if let Some(platform_config) = nixpkgs.per_platform.get(system) {
                if platform_config.allow_unfree {
                    config.allow_unfree = true;
                }
                if platform_config.allow_broken {
                    config.allow_broken = true;
                }
                if platform_config.cuda_support {
                    config.cuda_support = true;
                }
                if !platform_config.cuda_capabilities.is_empty() {
                    config.cuda_capabilities = platform_config.cuda_capabilities.clone();
                }
                if !platform_config.permitted_insecure_packages.is_empty() {
                    config.permitted_insecure_packages =
                        platform_config.permitted_insecure_packages.clone();
                }
                if !platform_config.permitted_unfree_packages.is_empty() {
                    config.permitted_unfree_packages =
                        platform_config.permitted_unfree_packages.clone();
                }
            }
        }

        config
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
    fn profile_field_none_by_default() {
        let config = Config::default();
        assert_eq!(config.profile, None);
    }

    #[test]
    fn profile_field_serializes() {
        let mut config = Config::default();
        config.profile = Some("production".to_string());

        let yaml = serde_yaml::to_string(&config).expect("Failed to serialize config");
        assert!(yaml.contains("profile: production"));
    }

    #[test]
    fn profile_field_not_serialized_when_none() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).expect("Failed to serialize config");
        assert!(!yaml.contains("profile"));
    }

    #[test]
    fn profile_field_respects_replace_merge_strategy() {
        let mut config1 = Config::default();
        config1.profile = Some("base".to_string());

        let mut config2 = Config::default();
        config2.profile = Some("override".to_string());

        // Simulating what schematic would do with replace merge strategy:
        // The second value should override the first
        let merged_profile = config2.profile.or(config1.profile);
        assert_eq!(merged_profile, Some("override".to_string()));
    }

    #[test]
    fn relative_path_url_resolved_from_correct_config_directory() {
        // Test that when a base config and imported config both define inputs
        // with relative path URLs like "path:.", each is resolved relative
        // to its own config directory, not confused with other directories.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let base_path = temp_dir.path();

        // Create subdirectory for import
        let subdir = base_path.join("subproject");
        std::fs::create_dir(&subdir).expect("Failed to create subdir");

        // Base config defines an input with path:.
        let base_config = r#"
inputs:
  base-local:
    url: path:.?dir=some-dir
imports:
  - ./subproject
"#;
        std::fs::write(base_path.join("devenv.yaml"), base_config)
            .expect("Failed to write base config");

        // Subproject config defines a different input with path:.
        // This should resolve to ./subproject, not confuse with base path
        let sub_config = r#"
inputs:
  sub-local:
    url: path:.
"#;
        std::fs::write(subdir.join("devenv.yaml"), sub_config).expect("Failed to write sub config");

        // Load the merged config
        let config = Config::load_from(base_path).expect("Failed to load config");

        // Verify the base-local input is resolved to "." (relative to base_path)
        let base_input = config
            .inputs
            .get("base-local")
            .expect("base-local not found");
        assert_eq!(
            base_input.url,
            Some("path:.?dir=some-dir".to_string()),
            "base-local should be normalized to path:. relative to base path"
        );

        // Verify sub-local is resolved to "./subproject" (relative to base_path)
        let sub_input = config.inputs.get("sub-local").expect("sub-local not found");
        assert_eq!(
            sub_input.url,
            Some("path:subproject".to_string()),
            "sub-local should be normalized to path:subproject relative to base path"
        );
    }

    #[test]
    fn absolute_path_outside_project_preserved() {
        // Test that absolute paths pointing outside the project directory
        // are preserved as absolute paths (not converted to relative paths).
        // This is necessary for lazy-trees mode in Nix.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let base_path = temp_dir.path();

        // Create the project directory
        let project_dir = base_path.join("project");
        std::fs::create_dir(&project_dir).expect("Failed to create project dir");

        // Create an external directory (sibling, not inside project)
        let external_dir = base_path.join("external");
        std::fs::create_dir(&external_dir).expect("Failed to create external dir");

        // Get the absolute path to the external directory
        let external_abs = external_dir
            .canonicalize()
            .expect("Failed to canonicalize external dir");

        // Config with an absolute path to the external directory
        let config_content = format!(
            r#"
inputs:
  external-input:
    url: path:{}
    flake: false
"#,
            external_abs.display()
        );
        std::fs::write(project_dir.join("devenv.yaml"), &config_content)
            .expect("Failed to write config");

        // Load the config
        let config = Config::load_from(&project_dir).expect("Failed to load config");

        // The absolute path should be preserved (not converted to "../external")
        let input = config
            .inputs
            .get("external-input")
            .expect("external-input not found");
        let url = input.url.as_ref().expect("URL should be set");

        // Should still be an absolute path, not a relative one
        assert!(
            url.starts_with("path:/"),
            "Absolute path outside project should be preserved as absolute, got: {}",
            url
        );
        assert!(
            !url.contains("../"),
            "Should not be converted to relative path with ../, got: {}",
            url
        );
    }
}
