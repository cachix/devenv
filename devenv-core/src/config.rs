use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pathdiff;
use schemars::JsonSchema;
use schematic::ConfigLoader;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

const YAML_CONFIG: &str = "devenv.yaml";
const YAML_LOCAL_CONFIG: &str = "devenv.local.yaml";

/// Version requirement for the devenv CLI.
///
/// - `true`: CLI version must match the modules version (checked during Nix evaluation)
/// - A constraint string like `">=2.0.0"`: checked before Nix evaluation
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, schematic::Schematic)]
#[serde(untagged)]
pub enum RequireVersion {
    /// When true, CLI version must match the modules version
    Match(bool),
    /// Version constraint string (e.g., ">=2.0.0", "2.0.7")
    Constraint(String),
}

/// Configure the built-in SecretSpec requirement for the Cachix auth token.
///
/// - `true`: require the default `CACHIX_AUTH_TOKEN` secret.
/// - `false`: disable SecretSpec lookup for the Cachix auth token.
/// - A string: require a secret with that name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, schematic::Schematic)]
#[serde(untagged)]
pub enum CachixAuthToken {
    Enabled(bool),
    Name(String),
}

impl CachixAuthToken {
    /// The SecretSpec lookup name, if lookup is enabled.
    pub fn secret_name(&self) -> Option<&str> {
        match self {
            Self::Enabled(true) => Some("CACHIX_AUTH_TOKEN"),
            Self::Enabled(false) => None,
            Self::Name(name) if !name.is_empty() => Some(name),
            Self::Name(_) => None,
        }
    }

    /// Whether a missing secret should trigger the built-in requirement.
    pub fn is_required(&self) -> bool {
        match self {
            Self::Enabled(enabled) => *enabled,
            Self::Name(name) => !name.is_empty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, schematic::Schematic)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum NixBackendType {
    #[default]
    Nix,
    #[cfg(feature = "snix")]
    Snix,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AndroidSdkConfig {
    /// Accept the Android SDK license.
    /// Can also be set via the `NIXPKGS_ACCEPT_ANDROID_SDK_LICENSE=1` environment variable.
    ///
    /// Default: `false`.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "acceptLicense", merge = schematic::merge::replace)]
    pub accept_license: bool,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct NixpkgsConfig {
    /// Allow unfree packages.
    ///
    /// Default: `false`.
    ///
    /// Added in 1.7.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowUnfree", merge = schematic::merge::replace)]
    pub allow_unfree: bool,
    /// Allow packages that are not supported on the current system.
    ///
    /// Default: `false`.
    ///
    /// Added in 2.0.5.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowUnsupportedSystem", merge = schematic::merge::replace)]
    pub allow_unsupported_system: bool,
    /// Allow packages marked as broken.
    ///
    /// Default: `false`.
    ///
    /// Added in 1.7.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowBroken", merge = schematic::merge::replace)]
    pub allow_broken: bool,
    /// Allow packages not built from source.
    ///
    /// Default: `true` (nixpkgs default).
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowNonSource", merge = schematic::merge::replace)]
    pub allow_non_source: bool,
    /// Enable CUDA support for nixpkgs.
    ///
    /// Default: `false`.
    ///
    /// Added in 1.7.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "cudaSupport", merge = schematic::merge::replace)]
    pub cuda_support: bool,
    /// Select CUDA capabilities for nixpkgs.
    ///
    /// Default: `[]`.
    ///
    /// Added in 1.7.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(alias = "cudaCapabilities", merge = schematic::merge::append_vec)]
    pub cuda_capabilities: Vec<String>,
    /// Enable ROCm support for nixpkgs.
    ///
    /// Default: `false`.
    ///
    /// Added in 2.0.7.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "rocmSupport", merge = schematic::merge::replace)]
    pub rocm_support: bool,
    /// A list of insecure permitted packages.
    ///
    /// Default: `[]`.
    ///
    /// Added in 1.7.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(alias = "permittedInsecurePackages", merge = schematic::merge::append_vec)]
    pub permitted_insecure_packages: Vec<String>,
    /// A list of unfree packages to allow by name.
    ///
    /// Default: `[]`.
    ///
    /// Added in 1.9.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(alias = "permittedUnfreePackages")]
    pub permitted_unfree_packages: Vec<String>,
    /// A list of license names to allow.
    /// Uses nixpkgs license attribute names (e.g. `gpl3Only`, `mit`, `asl20`).
    /// See [nixpkgs license list](https://github.com/NixOS/nixpkgs/blob/master/lib/licenses.nix).
    ///
    /// Default: `[]`.
    #[serde(skip_serializing, default)]
    #[setting(alias = "allowlistedLicenses", merge = schematic::merge::append_vec)]
    pub allowlisted_licenses: Vec<String>,
    /// A list of license names to block.
    /// Uses nixpkgs license attribute names (e.g. `unfree`, `bsl11`).
    /// See [nixpkgs license list](https://github.com/NixOS/nixpkgs/blob/master/lib/licenses.nix).
    ///
    /// Default: `[]`.
    #[serde(skip_serializing, default)]
    #[setting(alias = "blocklistedLicenses", merge = schematic::merge::append_vec)]
    pub blocklisted_licenses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(nested)]
    pub android_sdk: Option<AndroidSdkConfig>,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Input {
    /// URI specification of the input.
    /// See [Supported URI formats](../inputs.md#supported-uri-formats).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    /// Does the input contain `flake.nix` or `devenv.nix`.
    ///
    /// Default: `true`.
    #[serde(skip_serializing_if = "is_true", default = "true_default")]
    #[setting(default = true)]
    pub flake: bool,
    /// Another input to "inherit" from by name.
    /// See [Following inputs](../inputs.md#following-inputs).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub follows: Option<String>,
    /// Override nested inputs by name.
    /// See [Following inputs](../inputs.md#following-inputs).
    ///
    /// Opaque.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub inputs: BTreeMap<String, Input>,
    /// A list of overlays to include from the input.
    /// See [Overlays](../overlays.md).
    ///
    /// Default: `[]`.
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

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Clean {
    /// Clean the environment when entering the shell.
    ///
    /// Default: `false`.
    ///
    /// Added in 1.0.
    pub enabled: bool,
    /// A list of environment variables to keep when cleaning the environment.
    ///
    /// Default: `[]`.
    ///
    /// Added in 1.0.
    pub keep: Vec<String>,
    // TODO: executables?
}

impl Clean {
    /// Variables devenv requires internally and must survive env cleaning.
    /// `_DEVENV_HOOK_DIR` marks shells spawned by `devenv hook`; the shell
    /// hooks rely on its presence to know whether `cd`-ing out of the
    /// project should exit the current shell.
    const ALWAYS_KEEP: &'static [&'static str] = &["_DEVENV_HOOK_DIR"];

    /// Return host environment variables filtered by the clean/keep settings.
    ///
    /// When `enabled`, only variables whose name appears in `keep` or
    /// `ALWAYS_KEEP` are returned. Otherwise every host variable is returned.
    pub fn kept_env_vars(&self) -> HashMap<String, String> {
        let vars = std::env::vars();
        if self.enabled {
            let keep: HashSet<&str> = self
                .keep
                .iter()
                .map(|s| s.as_str())
                .chain(Self::ALWAYS_KEEP.iter().copied())
                .collect();
            vars.filter(|(key, _)| keep.contains(key.as_str()))
                .collect()
        } else {
            vars.collect()
        }
    }
}

#[derive(schematic::Config, Clone, Serialize, Debug, JsonSchema)]
#[config(allow_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub struct Nixpkgs {
    #[serde(flatten)]
    #[setting(nested)]
    pub config_: NixpkgsConfig,
    /// Per-platform nixpkgs configuration.
    /// Accepts the same options as `nixpkgs`.
    ///
    /// Opaque.
    ///
    /// Added in 1.7.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    // TODO(v3.0): remove deprecated alias
    #[setting(alias = "per-platform", nested, merge = schematic::merge::merge_btreemap)]
    pub per_platform: BTreeMap<String, NixpkgsConfig>,
}

#[derive(schematic::Config, Clone, Serialize, Debug, JsonSchema)]
#[config(allow_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    /// Version requirement for the devenv CLI.
    /// Set to `true` to enforce that the CLI version matches the modules version
    /// (from the `devenv` input), or use a constraint string with operators
    /// (`>=`, `<=`, `>`, `<`, `=`, or a bare version for an exact match).
    ///
    /// Added in 2.1.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(merge = schematic::merge::replace)]
    pub require_version: Option<RequireVersion>,
    /// Map of Nix inputs.
    /// See [Inputs](../inputs.md).
    ///
    /// Default: `inputs.nixpkgs.url: github:cachix/devenv-nixpkgs/rolling`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    #[setting(nested, merge = schematic::merge::merge_btreemap)]
    pub inputs: BTreeMap<String, Input>,
    // Deprecated top-level nixpkgs settings — deprecated since 2.0.
    // Read inside `nixpkgs_config()` under `#[allow(deprecated)]`.
    // TODO(v3.0): remove these fields and the `allow(deprecated)` shim.
    #[deprecated(since = "2.0.0", note = "use `nixpkgs.allow_unfree` instead")]
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowUnfree", merge = schematic::merge::replace)]
    pub allow_unfree: bool,
    #[deprecated(
        since = "2.0.0",
        note = "use `nixpkgs.allow_unsupported_system` instead"
    )]
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowUnsupportedSystem", merge = schematic::merge::replace)]
    pub allow_unsupported_system: bool,
    #[deprecated(since = "2.0.0", note = "use `nixpkgs.allow_broken` instead")]
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(alias = "allowBroken", merge = schematic::merge::replace)]
    pub allow_broken: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(nested)]
    pub nixpkgs: Option<Nixpkgs>,
    /// A list of relative paths, absolute paths, or references to inputs to import `devenv.nix` and `devenv.yaml` files.
    /// See [Composing using imports](../composing-using-imports.md).
    ///
    /// Default: `[]`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(merge = schematic::merge::append_vec)]
    pub imports: Vec<String>,
    #[deprecated(
        since = "2.0.0",
        note = "use `nixpkgs.permitted_insecure_packages` instead"
    )]
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[setting(alias = "permittedInsecurePackages", merge = schematic::merge::append_vec)]
    pub permitted_insecure_packages: Vec<String>,
    #[setting(nested)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub clean: Option<Clean>,
    /// Relax the hermeticity of the environment.
    ///
    /// Default: `false`.
    ///
    /// Added in 1.0.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(merge = schematic::merge::replace)]
    pub impure: bool,
    /// Select the Nix backend used to evaluate `devenv.nix`.
    ///
    /// Default: `nix`.
    #[serde(default, skip_serializing_if = "is_default")]
    #[setting(merge = schematic::merge::replace)]
    pub backend: NixBackendType,
    #[setting(nested)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub secretspec: Option<SecretspecConfig>,
    /// Default profile to activate.
    /// Can be overridden by `--profile` CLI flag.
    /// See [Profiles](../profiles.md).
    ///
    /// Added in 1.11.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(merge = schematic::merge::replace)]
    pub profile: Option<String>,
    /// Enable auto-reload of the shell when files change.
    /// Can be overridden by `--reload` or `--no-reload` CLI flags.
    ///
    /// Default: `true`.
    ///
    /// Added in 2.0.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(merge = schematic::merge::replace)]
    pub reload: Option<bool>,
    /// Error if a port is already in use instead of auto-allocating the next available port.
    /// Can be overridden by `--strict-ports` or `--no-strict-ports` CLI flags.
    ///
    /// Default: `false`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(alias = "strictPorts", merge = schematic::merge::replace)]
    pub strict_ports: Option<bool>,
    /// Default interactive shell to use when entering the devenv environment.
    /// Can be overridden by the `--shell` CLI flag.
    /// Falls back to the `$SHELL` environment variable, then `bash`.
    ///
    /// Supported values: `bash`, `zsh`, `fish`, `nu`. Any other value falls back to `bash`.
    ///
    /// Default: `$SHELL` or `bash`.
    ///
    /// Added in 2.1.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(merge = schematic::merge::replace)]
    pub shell: Option<String>,
    /// Git repository root path (not serialized, computed during load)
    #[serde(skip)]
    pub git_root: Option<PathBuf>,
}

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SecretspecConfig {
    /// Enable [secretspec integration](../integrations/secretspec.md).
    ///
    /// Default: `false`.
    ///
    /// Added in 1.8.
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    #[setting(default = false)]
    pub enable: bool,
    /// Secretspec profile name to use.
    ///
    /// Added in 1.8.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub profile: Option<String>,
    /// Secretspec provider to use.
    ///
    /// Added in 1.8.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider: Option<String>,
    /// Require the Cachix auth token through SecretSpec when
    /// `CACHIX_AUTH_TOKEN` is not set in the environment.
    ///
    /// Set to `true` to use the built-in `CACHIX_AUTH_TOKEN` secret name,
    /// `false` to disable SecretSpec lookup, or a string to use a custom
    /// secret name. No declaration in `secretspec.toml` is required.
    ///
    /// Default: unset.
    ///
    /// Added in 2.2.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cachix_auth_token: Option<CachixAuthToken>,
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

        // Collect imported yaml files only (not the base). These are merged
        // first, with the base loaded after them so base definitions take
        // precedence over imports.
        let mut imported_yamls = Vec::new();
        let mut visited = HashSet::new();

        if base_yaml.exists() {
            let canonical_base =
                base_yaml
                    .canonicalize()
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!("Failed to canonicalize base path: {}", base_yaml.display())
                    })?;
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

        // Recursively collect imported yaml files (loaded first, lowest priority)
        Self::collect_import_files(
            &temp_result.config.imports,
            base_path,
            git_root.as_deref(),
            &mut imported_yamls,
            &mut visited,
            0,
        )?;

        // Load imports first, then base last so base takes precedence.
        let load_order = imported_yamls
            .iter()
            .chain(base_yaml.exists().then_some(&base_yaml));

        // Load all configs and track which inputs come from which config file.
        // This is needed to correctly normalize relative URLs.
        let mut loader = ConfigLoader::<Config>::new();
        let mut input_source_dirs: HashMap<String, PathBuf> = HashMap::new();

        for yaml_file in load_order {
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

            // Record the source directory for each input defined in this config.
            // Later configs take precedence (base overrides imports).
            for input_name in single_result.config.inputs.keys() {
                input_source_dirs.insert(input_name.clone(), config_dir.clone());
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

        // Add all loaded file imports (normalized).
        for yaml_path in &imported_yamls {
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

    /// Check that `current` (e.g. "2.0.7") satisfies the version requirement in
    /// `devenv.yaml`. No-op when no requirement is set or when `require_version: true`
    /// (deferred to Nix evaluation where the modules version is available).
    pub fn check_version(&self, current: &str) -> Result<()> {
        let constraint = match &self.require_version {
            // Match(_) is either disabled (false) or deferred to Nix eval (true)
            None | Some(RequireVersion::Match(_)) => return Ok(()),
            Some(RequireVersion::Constraint(c)) => c,
        };

        // Bare version "2.0.7" means exact match; semver crate treats it as "^2.0.7"
        let req_str = if constraint.starts_with('>')
            || constraint.starts_with('<')
            || constraint.starts_with('=')
        {
            constraint.clone()
        } else {
            format!("={constraint}")
        };

        let cur = Self::parse_version(current)
            .wrap_err_with(|| format!("Failed to parse current devenv version '{current}'"))?;
        let req = VersionReq::parse(&req_str)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("Failed to parse version constraint '{constraint}' in devenv.yaml")
            })?;

        if !req.matches(&cur) {
            bail!(
                "devenv version {current} does not satisfy the constraint '{constraint}' in devenv.yaml"
            );
        }

        Ok(())
    }

    /// Returns true when `require_version: true` is set, meaning the Nix modules
    /// should assert that CLI version matches the modules version.
    pub fn requires_version_match(&self) -> bool {
        matches!(&self.require_version, Some(RequireVersion::Match(true)))
    }

    /// Parse a version string, accepting "X.Y" (appending ".0") and "X.Y.Z".
    fn parse_version(s: &str) -> Result<Version> {
        // semver crate requires X.Y.Z; support X.Y by appending .0
        let normalized = if s.matches('.').count() == 1 {
            format!("{s}.0")
        } else {
            s.to_string()
        };
        Version::parse(&normalized)
            .into_diagnostic()
            .wrap_err_with(|| format!("expected version format X.Y or X.Y.Z, got '{s}'"))
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
        let canonical_root = match security_root.canonicalize() {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // The path will be validated when the security root exists.
                return Ok(());
            }
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!(
                        "Failed to canonicalize security root {}",
                        security_root.display()
                    )
                });
            }
        };

        let abs_import = match import_path.canonicalize() {
            Ok(import) => import,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                // Soft-canonicalize a missing import so the prefix comparison
                // against the canonicalized root isn't defeated by a symlinked
                // base path (e.g. /var -> /private/var on macOS).
                // NotADirectory is a flavor of missing: input-style imports
                // (e.g. `foo/bar` for input `foo`) may collide with a regular
                // file named `foo` in the project root.
                let import = if import_path.is_absolute() {
                    import_path.to_path_buf()
                } else {
                    std::env::current_dir()
                        .into_diagnostic()
                        .wrap_err("Failed to get current directory")?
                        .join(import_path)
                };
                Self::soft_canonicalize(&import)?
            }
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!(
                        "Failed to canonicalize import path {}",
                        import_path.display()
                    )
                });
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

        Ok(())
    }

    /// Resolves an absolute path that may not exist. Existing components are
    /// cheaply inspected for symlinks; after the first missing component, the
    /// suffix is processed lexically without further filesystem access. If `..`
    /// returns to the existing prefix, filesystem inspection resumes.
    ///
    /// Resolving symlinks before applying `..` is important: `link/..` means
    /// "parent of the link target", which lexical normalization would get wrong.
    fn soft_canonicalize(path: &Path) -> Result<PathBuf> {
        let mut symlinks_followed = 0;
        let (resolved, _) = Self::soft_canonicalize_inner(path, &mut symlinks_followed)?;
        Ok(resolved)
    }

    /// Returns the resolved path and the number of unresolved trailing
    /// components. Keeping the depth lets `missing/..` return to the known
    /// existing prefix without making redundant filesystem calls.
    fn soft_canonicalize_inner(
        path: &Path,
        symlinks_followed: &mut usize,
    ) -> Result<(PathBuf, usize)> {
        use std::path::Component;

        const MAX_SYMLINKS: usize = 40;

        let mut resolved = PathBuf::new();
        let mut unresolved_depth: usize = 0;

        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => resolved.push(component),
                Component::CurDir => {}
                Component::ParentDir => {
                    resolved.pop();
                    unresolved_depth = unresolved_depth.saturating_sub(1);
                }
                Component::Normal(name) if unresolved_depth > 0 => {
                    resolved.push(name);
                    unresolved_depth += 1;
                }
                Component::Normal(name) => {
                    let candidate = resolved.join(name);

                    match std::fs::symlink_metadata(&candidate) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            *symlinks_followed += 1;
                            if *symlinks_followed > MAX_SYMLINKS {
                                bail!(
                                    "Too many levels of symbolic links while resolving {}",
                                    candidate.display()
                                );
                            }

                            // The metadata lookup already established that this
                            // is a symlink. Reading and resolving its target
                            // directly avoids a redundant full canonicalize.
                            let target = match std::fs::read_link(&candidate) {
                                Ok(target) => target,
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                    // The link disappeared between the metadata
                                    // lookup and read_link; treat it as the first
                                    // missing component.
                                    resolved.push(name);
                                    unresolved_depth = 1;
                                    continue;
                                }
                                Err(error) => {
                                    return Err(error).into_diagnostic().wrap_err_with(|| {
                                        format!(
                                            "Failed to read symbolic link {}",
                                            candidate.display()
                                        )
                                    });
                                }
                            };
                            let target = if target.is_absolute() {
                                target
                            } else {
                                resolved.join(target)
                            };
                            (resolved, unresolved_depth) =
                                Self::soft_canonicalize_inner(&target, symlinks_followed)?;
                        }
                        Ok(_) => resolved.push(name),
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                            ) =>
                        {
                            resolved.push(name);
                            unresolved_depth = 1;
                        }
                        Err(error) => {
                            return Err(error).into_diagnostic().wrap_err_with(|| {
                                format!("Failed to inspect path component {}", candidate.display())
                            });
                        }
                    }
                }
            }
        }

        Ok((resolved, unresolved_depth))
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

    pub fn write(&self) -> Result<()> {
        let root_dir = std::env::current_dir()
            .into_diagnostic()
            .wrap_err("Failed to get current directory")?;
        self.write_to(&root_dir)
    }

    fn write_to(&self, root_dir: &Path) -> Result<()> {
        let yaml = serde_yaml::to_string(&self)
            .into_diagnostic()
            .wrap_err("Failed to serialize config to YAML")?;
        let content = format!(
            "# yaml-language-server: $schema=https://devenv.sh/devenv.schema.json\n{}",
            yaml
        );
        std::fs::write(root_dir.join(YAML_CONFIG), content)
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
    /// Layers (lowest to highest priority):
    /// 1. Deprecated top-level fields (`allow_unfree`, etc.).
    /// 2. `nixpkgs.<field>`.
    /// 3. `nixpkgs.per_platform.<system>.<field>`.
    ///
    /// Merging uses schematic's `#[setting(merge)]` strategies declared on
    /// `NixpkgsConfig` — same policy as the import-time merge in
    /// `ConfigLoader`. `Vec` fields accumulate via `append_vec`, scalars use
    /// `replace`. No special platform-only semantics.
    pub fn nixpkgs_config(&self, system: &str) -> NixpkgsConfig {
        use schematic::{Config as _, PartialConfig as _};

        // Layer 1: deprecated top-level fields.
        #[allow(deprecated)]
        let mut partial = PartialNixpkgsConfig {
            allow_unfree: Some(self.allow_unfree),
            allow_unsupported_system: Some(self.allow_unsupported_system),
            allow_broken: Some(self.allow_broken),
            permitted_insecure_packages: Some(self.permitted_insecure_packages.clone()),
            ..Default::default()
        };

        if let Some(nixpkgs) = &self.nixpkgs {
            // Layer 2: `nixpkgs.<field>`.
            partial
                .merge(&(), nixpkgs_to_partial(&nixpkgs.config_))
                .expect("merge base nixpkgs config");
            // Layer 3: `nixpkgs.per_platform.<system>.<field>`.
            if let Some(platform) = nixpkgs.per_platform.get(system) {
                partial
                    .merge(&(), nixpkgs_to_partial(platform))
                    .expect("merge per-platform nixpkgs config");
            }
        }

        NixpkgsConfig::from_partial(
            partial
                .finalize(&())
                .expect("finalize nixpkgs partial config"),
        )
    }
}

/// Project a [`NixpkgsConfig`] into its schematic Partial form so it can be
/// merged via [`schematic::PartialConfig::merge`].
fn nixpkgs_to_partial(c: &NixpkgsConfig) -> PartialNixpkgsConfig {
    // Destructure so adding a field forces an update here.
    let NixpkgsConfig {
        allow_unfree,
        allow_unsupported_system,
        allow_broken,
        allow_non_source,
        cuda_support,
        cuda_capabilities,
        rocm_support,
        permitted_insecure_packages,
        permitted_unfree_packages,
        allowlisted_licenses,
        blocklisted_licenses,
        android_sdk,
    } = c.clone();
    PartialNixpkgsConfig {
        allow_unfree: Some(allow_unfree),
        allow_unsupported_system: Some(allow_unsupported_system),
        allow_broken: Some(allow_broken),
        allow_non_source: Some(allow_non_source),
        cuda_support: Some(cuda_support),
        cuda_capabilities: Some(cuda_capabilities),
        rocm_support: Some(rocm_support),
        permitted_insecure_packages: Some(permitted_insecure_packages),
        permitted_unfree_packages: Some(permitted_unfree_packages),
        allowlisted_licenses: Some(allowlisted_licenses),
        blocklisted_licenses: Some(blocklisted_licenses),
        android_sdk: android_sdk.map(|sdk| PartialAndroidSdkConfig {
            accept_license: Some(sdk.accept_license),
        }),
    }
}

// Clap helpers

fn true_default() -> bool {
    true
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
    fn add_input_with_missing_follows() {
        let mut config = Config::default();
        let result = config.add_input(
            "input-with-follows",
            "github:org/repo",
            &["other".to_string()],
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Input other does not exist so it can't be followed."),
            "unexpected error: {err}"
        );
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
    fn strict_ports_field_none_by_default() {
        let config = Config::default();
        assert_eq!(config.strict_ports, None);
    }

    #[test]
    fn strict_ports_field_serializes_as_snake_case() {
        let mut config = Config::default();
        config.strict_ports = Some(true);

        let yaml = serde_yaml::to_string(&config).expect("Failed to serialize config");
        assert!(yaml.contains("strict_ports: true"));
    }

    #[test]
    fn strict_ports_field_not_serialized_when_none() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).expect("Failed to serialize config");
        assert!(!yaml.contains("strict_ports"));
    }

    #[test]
    fn strict_ports_field_respects_replace_merge_strategy() {
        let mut config1 = Config::default();
        config1.strict_ports = Some(false);

        let mut config2 = Config::default();
        config2.strict_ports = Some(true);

        let merged_strict_ports = config2.strict_ports.or(config1.strict_ports);
        assert_eq!(merged_strict_ports, Some(true));
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
        fs::create_dir(&subdir).expect("Failed to create subdir");

        // Base config defines an input with path:.
        let base_config = r#"
inputs:
  base-local:
    url: path:.?dir=some-dir
imports:
  - ./subproject
"#;
        fs::write(base_path.join("devenv.yaml"), base_config).expect("Failed to write base config");

        // Subproject config defines a different input with path:.
        // This should resolve to ./subproject, not confuse with base path
        let sub_config = r#"
inputs:
  sub-local:
    url: path:.
"#;
        fs::write(subdir.join("devenv.yaml"), sub_config).expect("Failed to write sub config");

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
        fs::create_dir(&project_dir).expect("Failed to create project dir");

        // Create an external directory (sibling, not inside project)
        let external_dir = base_path.join("external");
        fs::create_dir(&external_dir).expect("Failed to create external dir");

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
        fs::write(project_dir.join("devenv.yaml"), &config_content)
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

    #[test]
    fn imported_config_does_not_override_base_inputs() {
        // When a sub project imports a shared config and both define the same
        // input, the sub project's (base) definition should take precedence.
        // Regression test for https://github.com/cachix/devenv/issues/2728
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir.path();

        // Initialize a git repo so import security checks pass
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .expect("Failed to git init");

        // Shared config defines nixpkgs with the default URL
        let shared_dir = root.join("shared");
        fs::create_dir(&shared_dir).expect("Failed to create shared dir");
        fs::write(
            shared_dir.join("devenv.yaml"),
            r#"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
"#,
        )
        .expect("Failed to write shared config");

        // Sub project imports shared and overrides nixpkgs
        let sub_dir = root.join("sub");
        fs::create_dir(&sub_dir).expect("Failed to create sub dir");
        fs::write(
            sub_dir.join("devenv.yaml"),
            r#"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-25.11

imports:
  - ../shared
"#,
        )
        .expect("Failed to write sub config");

        let config = Config::load_from(&sub_dir).expect("Failed to load config");

        let nixpkgs = config.inputs.get("nixpkgs").expect("nixpkgs not found");
        assert_eq!(
            nixpkgs.url,
            Some("github:NixOS/nixpkgs/nixos-25.11".to_string()),
            "Base config's nixpkgs URL should take precedence over imported config's URL"
        );
    }

    #[test]
    fn sub_import_with_yaml_does_not_duplicate_base_import() {
        // When a sub project imports and has a devenv.yaml (even empty),
        // the base directory should NOT end up in the imports list.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir.path();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .expect("Failed to git init");

        let sub_dir = root.join("sub");
        fs::create_dir(&sub_dir).expect("Failed to create sub dir");
        fs::write(sub_dir.join("devenv.yaml"), "").expect("Failed to write sub yaml");

        fs::write(
            root.join("devenv.yaml"),
            r#"
imports:
  - ./sub
"#,
        )
        .expect("Failed to write base yaml");

        let config = Config::load_from(root).expect("Failed to load config");

        assert!(
            !config
                .imports
                .iter()
                .any(|i| i == "./" || i == "." || i == "./."),
            "Base directory should not appear in final_imports, got: {:?}",
            config.imports
        );
    }

    #[cfg(unix)]
    #[test]
    fn input_import_under_symlinked_root_is_not_rejected() {
        // Regression test for input-style imports (e.g. "simple/examples/simple")
        // when the project root is reached through a symlink. On macOS the system
        // temp dir lives under the /var -> /private/var symlink: git reports the
        // physical root while the base path stays logical. The import path does
        // not exist on disk, so validation fell back to lexical normalization of
        // the logical path and wrongly rejected the import as escaping the repo.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let real_root = temp_dir.path().join("real");
        fs::create_dir(&real_root).expect("Failed to create real root");
        let linked_root = temp_dir.path().join("link");
        std::os::unix::fs::symlink(&real_root, &linked_root).expect("Failed to create symlink");

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&real_root)
            .output()
            .expect("Failed to git init");

        fs::write(
            real_root.join("devenv.yaml"),
            r#"
inputs:
  simple:
    url: github:cachix/devenv
    flake: false
imports:
  - simple/examples/simple
"#,
        )
        .expect("Failed to write config");

        let config = Config::load_from(&linked_root)
            .expect("Input-style import should not be rejected under a symlinked root");
        assert!(
            config.imports.iter().any(|i| i == "simple/examples/simple"),
            "Input import should be preserved, got: {:?}",
            config.imports
        );
    }

    #[test]
    fn input_import_colliding_with_regular_file_is_not_rejected() {
        // Regression test for an input-style import (e.g. "foo/bar" for input
        // `foo`) when a regular file named `foo` exists in the project root.
        // Canonicalizing the import then fails with NotADirectory rather than
        // NotFound, which must be treated as a missing path, not a hard error.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir.path().join("project");
        fs::create_dir(&root).expect("Failed to create project dir");

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .expect("Failed to git init");

        fs::write(root.join("foo"), "").expect("Failed to create conflicting file");

        fs::write(
            root.join("devenv.yaml"),
            r#"
inputs:
  foo:
    url: github:cachix/devenv
    flake: false
imports:
  - foo/bar
"#,
        )
        .expect("Failed to write config");

        let config = Config::load_from(&root)
            .expect("Input-style import colliding with a regular file should not be rejected");
        assert!(
            config.imports.iter().any(|i| i == "foo/bar"),
            "Input import should be preserved, got: {:?}",
            config.imports
        );
    }

    #[cfg(unix)]
    #[test]
    fn nonexistent_import_escaping_via_symlink_and_dotdot_is_rejected() {
        // `..` must be resolved against the real filesystem, not lexically:
        // with `link -> <outside>`, the import `./link/../missing` resolves to
        // a sibling of the symlink *target* (outside the repo), while lexical
        // normalization would collapse it to `<repo>/missing` (inside).
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let outside = temp_dir.path().join("outside");
        fs::create_dir(&outside).expect("Failed to create outside dir");
        let repo = temp_dir.path().join("repo");
        fs::create_dir(&repo).expect("Failed to create repo dir");
        std::os::unix::fs::symlink(&outside, repo.join("link")).expect("Failed to create symlink");

        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo)
            .status()
            .expect("Failed to run git init");
        assert!(status.success(), "git init failed");

        fs::write(
            repo.join("devenv.yaml"),
            r#"
imports:
  - ./link/../missing
"#,
        )
        .expect("Failed to write config");

        let err = Config::load_from(&repo)
            .expect_err("Import escaping the repo through a symlink should be rejected");
        assert!(
            err.to_string().contains("resolves outside"),
            "Expected path traversal error, got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn soft_canonicalize_resolves_broken_symlink_target_before_dotdot() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let repo = temp_dir.path().join("repo");
        let outside = temp_dir.path().join("outside");
        fs::create_dir(&repo).expect("Failed to create repo dir");
        fs::create_dir(&outside).expect("Failed to create outside dir");
        std::os::unix::fs::symlink(outside.join("missing-target"), repo.join("link"))
            .expect("Failed to create broken symlink");

        let resolved = Config::soft_canonicalize(&repo.join("link/../missing"))
            .expect("Broken symlink target should be soft-canonicalized");
        assert_eq!(
            resolved,
            outside
                .canonicalize()
                .expect("Failed to canonicalize outside dir")
                .join("missing")
        );
    }

    #[cfg(unix)]
    #[test]
    fn soft_canonicalize_rejects_symlink_loops() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let first = temp_dir.path().join("first");
        let second = temp_dir.path().join("second");
        std::os::unix::fs::symlink(&second, &first).expect("Failed to create first symlink");
        std::os::unix::fs::symlink(&first, &second).expect("Failed to create second symlink");

        let err = Config::soft_canonicalize(&first)
            .expect_err("A symbolic link loop should not be resolved");
        assert!(
            err.to_string()
                .contains("Too many levels of symbolic links"),
            "Expected a symbolic link depth error, got: {err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn soft_canonicalize_tolerates_missing_and_non_directory_components() {
        // NotFound and NotADirectory both mean "the path doesn't exist as a
        // directory tree" and start the lexical suffix; other errors (loops,
        // permissions) still fail because containment can't be established.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize temp dir");

        assert_eq!(
            Config::soft_canonicalize(&root.join("missing/deeper"))
                .expect("A missing suffix should be accepted"),
            root.join("missing/deeper")
        );

        let file = root.join("file");
        fs::write(&file, "").expect("Failed to create file");
        assert_eq!(
            Config::soft_canonicalize(&file.join("child"))
                .expect("A non-directory component should be treated as missing"),
            file.join("child")
        );
    }

    #[test]
    fn nonexistent_import_escaping_root_is_rejected() {
        // A non-existent import that lexically escapes the git root must still
        // be rejected by the fallback validation path.
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir.path().join("project");
        fs::create_dir(&root).expect("Failed to create project dir");

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .expect("Failed to git init");

        fs::write(
            root.join("devenv.yaml"),
            r#"
imports:
  - ../outside/nonexistent
"#,
        )
        .expect("Failed to write config");

        let err = Config::load_from(&root).expect_err("Escaping import should be rejected");
        assert!(
            err.to_string().contains("resolves outside"),
            "Expected path traversal error, got: {err}"
        );
    }

    #[test]
    fn check_version_none_always_passes() {
        let config = Config::default();
        assert!(config.check_version("2.0.7").is_ok());
    }

    #[test]
    fn check_version_false_always_passes() {
        let config = Config {
            require_version: Some(RequireVersion::Match(false)),
            ..Default::default()
        };
        assert!(config.check_version("2.0.7").is_ok());
    }

    #[test]
    fn check_version_true_deferred_to_nix() {
        let config = Config {
            require_version: Some(RequireVersion::Match(true)),
            ..Default::default()
        };
        // `true` is checked during Nix evaluation, so Rust check always passes
        assert!(config.check_version("2.0.7").is_ok());
        assert!(config.requires_version_match());
    }

    #[test]
    fn check_version_exact_match() {
        let config = Config {
            require_version: Some(RequireVersion::Constraint("2.0.7".to_string())),
            ..Default::default()
        };
        assert!(config.check_version("2.0.7").is_ok());
        assert!(config.check_version("2.0.8").is_err());
    }

    #[test]
    fn check_version_gte() {
        let config = Config {
            require_version: Some(RequireVersion::Constraint(">=2.0.0".to_string())),
            ..Default::default()
        };
        assert!(config.check_version("2.0.0").is_ok());
        assert!(config.check_version("2.0.7").is_ok());
        assert!(config.check_version("3.0.0").is_ok());
        assert!(config.check_version("1.9.9").is_err());
    }

    #[test]
    fn check_version_lt() {
        let config = Config {
            require_version: Some(RequireVersion::Constraint("<3.0.0".to_string())),
            ..Default::default()
        };
        assert!(config.check_version("2.0.7").is_ok());
        assert!(config.check_version("3.0.0").is_err());
    }

    #[test]
    fn check_version_two_component() {
        let config = Config {
            require_version: Some(RequireVersion::Constraint(">=2.1".to_string())),
            ..Default::default()
        };
        assert!(config.check_version("2.1.0").is_ok());
        assert!(config.check_version("2.1.5").is_ok());
        assert!(config.check_version("2.0.9").is_err());
    }

    #[test]
    fn check_version_invalid_constraint() {
        let config = Config {
            require_version: Some(RequireVersion::Constraint(">=abc".to_string())),
            ..Default::default()
        };
        assert!(config.check_version("2.0.7").is_err());
    }

    #[test]
    fn check_version_invalid_current() {
        let config = Config {
            require_version: Some(RequireVersion::Constraint(">=2.0.0".to_string())),
            ..Default::default()
        };
        assert!(config.check_version("not-a-version").is_err());
    }

    #[test]
    fn require_version_yaml_bool() {
        let yaml = "require_version: true\n";
        let parsed: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let rv: RequireVersion = serde_yaml::from_value(parsed["require_version"].clone()).unwrap();
        assert_eq!(rv, RequireVersion::Match(true));
    }

    #[test]
    fn require_version_yaml_string() {
        let yaml = "require_version: \">=2.0.0\"\n";
        let parsed: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let rv: RequireVersion = serde_yaml::from_value(parsed["require_version"].clone()).unwrap();
        assert_eq!(rv, RequireVersion::Constraint(">=2.0.0".to_string()));
    }

    #[test]
    fn cachix_auth_token_yaml_bool() {
        let enabled: CachixAuthToken = serde_yaml::from_str("true").unwrap();
        assert_eq!(enabled, CachixAuthToken::Enabled(true));
        assert_eq!(enabled.secret_name(), Some("CACHIX_AUTH_TOKEN"));
        assert!(enabled.is_required());

        let disabled: CachixAuthToken = serde_yaml::from_str("false").unwrap();
        assert_eq!(disabled, CachixAuthToken::Enabled(false));
        assert_eq!(disabled.secret_name(), None);
        assert!(!disabled.is_required());
    }

    #[test]
    fn cachix_auth_token_yaml_string() {
        let setting: CachixAuthToken = serde_yaml::from_str("MY_TEAM_CACHIX_TOKEN").unwrap();
        assert_eq!(
            setting,
            CachixAuthToken::Name("MY_TEAM_CACHIX_TOKEN".to_string())
        );
        assert_eq!(setting.secret_name(), Some("MY_TEAM_CACHIX_TOKEN"));
        assert!(setting.is_required());
    }

    #[test]
    fn cachix_auth_token_loads_through_devenv_config() {
        let enabled = load_yaml("secretspec:\n  enable: true\n  cachix_auth_token: true\n");
        assert_eq!(
            enabled.secretspec.unwrap().cachix_auth_token,
            Some(CachixAuthToken::Enabled(true))
        );

        let named =
            load_yaml("secretspec:\n  enable: true\n  cachix_auth_token: MY_TEAM_CACHIX_TOKEN\n");
        assert_eq!(
            named.secretspec.unwrap().cachix_auth_token,
            Some(CachixAuthToken::Name("MY_TEAM_CACHIX_TOKEN".to_string()))
        );
    }

    fn load_yaml(yaml: &str) -> Config {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        fs::write(temp_dir.path().join("devenv.yaml"), yaml).expect("Failed to write devenv.yaml");
        Config::load_from(temp_dir.path()).expect("Failed to load config")
    }

    const SNAKE_CASE_INPUT: &str = r#"
allow_unfree: true
allow_broken: true
allow_unsupported_system: true
permitted_insecure_packages: ["pkg1"]
strict_ports: true
nixpkgs:
  cuda_support: true
  cuda_capabilities: ["8.0"]
  rocm_support: true
  allow_non_source: true
  permitted_unfree_packages: ["terraform"]
  allowlisted_licenses: ["mit"]
  blocklisted_licenses: ["unfree"]
  android_sdk:
    accept_license: true
  per_platform:
    x86_64-linux:
      allow_broken: true
"#;

    const CAMELCASE_INPUT: &str = r#"
allowUnfree: true
allowBroken: true
allowUnsupportedSystem: true
permittedInsecurePackages: ["pkg1"]
strictPorts: true
nixpkgs:
  cudaSupport: true
  cudaCapabilities: ["8.0"]
  rocmSupport: true
  allowNonSource: true
  permittedUnfreePackages: ["terraform"]
  allowlistedLicenses: ["mit"]
  blocklistedLicenses: ["unfree"]
  android_sdk:
    acceptLicense: true
  per-platform:
    x86_64-linux:
      allowBroken: true
"#;

    // devenv.yaml input contract: snake_case parses, serializes pure snake_case.
    #[test]
    fn devenv_yaml_snake_case_parses() {
        let cfg = load_yaml(SNAKE_CASE_INPUT);
        let expected: serde_yaml::Value = serde_yaml::from_str(
            r#"
allow_unfree: true
allow_unsupported_system: true
allow_broken: true
nixpkgs:
  allow_non_source: true
  cuda_support: true
  cuda_capabilities: ["8.0"]
  rocm_support: true
  permitted_unfree_packages: ["terraform"]
  android_sdk:
    accept_license: true
  per_platform:
    x86_64-linux:
      allow_broken: true
permitted_insecure_packages: ["pkg1"]
strict_ports: true
"#,
        )
        .unwrap();
        assert_eq!(serde_yaml::to_value(&cfg).unwrap(), expected);

        // skip_serializing fields don't appear in Value; check directly.
        let nixpkgs = cfg.nixpkgs.unwrap();
        assert_eq!(nixpkgs.config_.allowlisted_licenses, vec!["mit"]);
        assert_eq!(nixpkgs.config_.blocklisted_licenses, vec!["unfree"]);
    }

    // devenv.yaml input contract: camelCase aliases load equivalently to snake_case.
    // TODO(v3.0): remove together with the camelCase aliases.
    #[test]
    fn devenv_yaml_camelcase_aliases_back_compat() {
        let snake = load_yaml(SNAKE_CASE_INPUT);
        let camel = load_yaml(CAMELCASE_INPUT);
        assert_eq!(
            serde_yaml::to_value(&camel).unwrap(),
            serde_yaml::to_value(&snake).unwrap()
        );
        // skip_serializing fields don't appear in Value; check directly.
        let snake_nixpkgs = snake.nixpkgs.unwrap();
        let camel_nixpkgs = camel.nixpkgs.unwrap();
        assert_eq!(
            camel_nixpkgs.config_.allowlisted_licenses,
            snake_nixpkgs.config_.allowlisted_licenses
        );
        assert_eq!(
            camel_nixpkgs.config_.blocklisted_licenses,
            snake_nixpkgs.config_.blocklisted_licenses
        );
    }

    // Platform merge: Vec fields accumulate across layers (deprecated top-level
    // → nixpkgs base → per_platform.<system>). Same semantics as imports merge.
    #[test]
    fn nixpkgs_config_platform_merge_appends_vecs() {
        let yaml = r#"
permitted_insecure_packages: ["from-top"]
nixpkgs:
  cuda_capabilities: ["7.5"]
  permitted_insecure_packages: ["from-base"]
  per_platform:
    x86_64-linux:
      cuda_capabilities: ["8.0"]
      permitted_insecure_packages: ["from-platform"]
"#;
        let cfg = load_yaml(yaml).nixpkgs_config("x86_64-linux");
        assert_eq!(cfg.cuda_capabilities, vec!["7.5", "8.0"]);
        assert_eq!(
            cfg.permitted_insecure_packages,
            vec!["from-top", "from-base", "from-platform"]
        );
    }

    // Platform merge: bool fields OR across layers (any true wins).
    #[test]
    fn nixpkgs_config_platform_merge_ors_bools() {
        let yaml = r#"
nixpkgs:
  allow_unfree: false
  per_platform:
    x86_64-linux:
      allow_unfree: true
"#;
        assert!(load_yaml(yaml).nixpkgs_config("x86_64-linux").allow_unfree);
    }

    // Deprecated top-level fields still feed the platform merge (lowest layer).
    #[test]
    fn nixpkgs_config_reads_deprecated_top_level_fields() {
        let yaml = r#"
allow_unfree: true
permitted_insecure_packages: ["from-top"]
"#;
        let cfg = load_yaml(yaml).nixpkgs_config("x86_64-linux");
        assert!(cfg.allow_unfree);
        assert_eq!(cfg.permitted_insecure_packages, vec!["from-top"]);
    }

    #[test]
    fn config_write_includes_schema_comment() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let base_path = temp_dir.path();

        let mut config = Config::default();
        config
            .add_input("nixpkgs-python", "github:cachix/nixpkgs-python", &[])
            .expect("Failed to add an input to config");
        config
            .write_to(base_path)
            .expect("Failed to write config file");

        let yaml_path = base_path.join(YAML_CONFIG);
        let read_content = fs::read_to_string(&yaml_path).expect("Failed to read devenv.yaml");
        assert!(
            read_content.starts_with(
                "# yaml-language-server: $schema=https://devenv.sh/devenv.schema.json\n"
            ),
            "Config file should start with schema comment, but got: {}",
            read_content
        );
    }
}
