use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pathdiff;
use schemars::{JsonSchema, schema_for};
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
    /// Return host environment variables filtered by the clean/keep settings.
    ///
    /// When `enabled`, only variables whose name appears in `keep` are
    /// returned. Otherwise every host variable is returned.
    pub fn kept_env_vars(&self) -> HashMap<String, String> {
        let vars = std::env::vars();
        if self.enabled {
            let keep: HashSet<&str> = self.keep.iter().map(|s| s.as_str()).collect();
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

pub async fn write_yaml_options_doc() -> Result<()> {
    let schema = schema_for!(Config);
    let json: serde_json::Value = serde_json::to_value(&schema)
        .into_diagnostic()
        .wrap_err("Failed to serialize JSON schema")?;
    let rendered = render_yaml_options(&json);
    let path = Path::new("docs/src/reference/yaml-options.md");
    tokio::fs::write(path, &rendered)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to write yaml-options to {}", path.display()))?;
    Ok(())
}

struct OptionSection {
    path: String,
    description: String,
    added_in: Option<String>,
    default: Option<String>,
    type_label: String,
}

struct ParsedMeta {
    body: String,
    added_in: Option<String>,
    default: Option<String>,
    opaque: bool,
}

fn render_yaml_options(schema: &serde_json::Value) -> String {
    let defs = schema
        .get("$defs")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let mut sections: Vec<OptionSection> = Vec::new();
    let visited: HashSet<String> = HashSet::new();
    for (name, prop) in properties.iter() {
        collect_sections(name, prop, &defs, &visited, &mut sections);
    }
    sections.sort_by(|a, b| a.path.cmp(&b.path));

    let mut out = String::from("# devenv.yaml\n\n");
    out.push_str("<!-- This file is auto-generated from devenv-core/src/config.rs doc comments. Do not edit. -->\n\n");
    for section in sections {
        out.push_str(&render_section(&section));
    }
    out
}

/// Extract the `$ref` target name from a schema, considering direct `$ref` and `anyOf` wrappers.
fn ref_target(schema: &serde_json::Value) -> Option<String> {
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str())
        && let Some(name) = r.strip_prefix("#/$defs/")
    {
        return Some(name.to_string());
    }
    if let Some(any) = schema.get("anyOf").and_then(|v| v.as_array()) {
        let non_null: Vec<&serde_json::Value> = any
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
            .collect();
        if non_null.len() == 1 {
            return ref_target(non_null[0]);
        }
    }
    None
}

fn collect_sections(
    path: &str,
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
    visited: &HashSet<String>,
    out: &mut Vec<OptionSection>,
) {
    if schema.get("deprecated").and_then(|v| v.as_bool()) == Some(true) {
        return;
    }

    let resolved = resolve_ref(schema, defs);
    let description = description_of(schema).unwrap_or_default();
    let opaque = parse_description(&description).opaque;

    // Map types (BTreeMap<String, T>) -> emit "<path>.<name>.<sub>" sections via additionalProperties.
    if let Some(additional) = resolved.get("additionalProperties") {
        let wildcard_path = format!("{}.\\<name\\>", path);
        let inner_ref = ref_target(additional);
        let cycle = inner_ref
            .as_ref()
            .map(|name| visited.contains(name))
            .unwrap_or(false);
        let inner_resolved = resolve_ref(additional, defs);

        if !opaque
            && !cycle
            && inner_resolved
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some()
        {
            if !description.is_empty() {
                out.push(make_section(
                    path,
                    &type_label(&resolved, defs),
                    description,
                ));
            }
            let mut next = visited.clone();
            if let Some(name) = inner_ref {
                next.insert(name);
            }
            if let Some(props) = inner_resolved.get("properties").and_then(|v| v.as_object()) {
                for (name, sub) in props {
                    collect_sections(
                        &format!("{}.{}", wildcard_path, name),
                        sub,
                        defs,
                        &next,
                        out,
                    );
                }
            }
            return;
        }
        // Cycle, opaque, scalar value type, or no struct properties -> single section.
        out.push(make_section(
            path,
            &type_label(&resolved, defs),
            description,
        ));
        return;
    }

    // Object with properties (via $ref or inline) -> recurse.
    let inline_ref = ref_target(schema);
    let cycle = inline_ref
        .as_ref()
        .map(|name| visited.contains(name))
        .unwrap_or(false);
    if !opaque
        && !cycle
        && let Some(props) = resolved.get("properties").and_then(|v| v.as_object())
    {
        if !description.is_empty() {
            out.push(make_section(
                path,
                &type_label(&resolved, defs),
                description,
            ));
        }
        let mut next = visited.clone();
        if let Some(name) = inline_ref {
            next.insert(name);
        }
        for (name, sub) in props {
            collect_sections(&format!("{}.{}", path, name), sub, defs, &next, out);
        }
        return;
    }

    // Leaf scalar / enum / cycle / opaque.
    let desc = if description.is_empty() {
        description_of(&resolved).unwrap_or_default()
    } else {
        description
    };
    out.push(make_section(path, &type_label(&resolved, defs), desc));
}

fn make_section(path: &str, type_label: &str, raw_description: String) -> OptionSection {
    let meta = parse_description(&raw_description);
    OptionSection {
        path: path.to_string(),
        description: meta.body,
        added_in: meta.added_in,
        default: meta.default,
        type_label: type_label.to_string(),
    }
}

fn parse_description(input: &str) -> ParsedMeta {
    let mut lines: Vec<String> = input.lines().map(|l| l.to_string()).collect();
    let mut added_in: Option<String> = None;
    let mut default: Option<String> = None;
    let mut opaque = false;
    // Walk lines from the end, pulling off trailing metadata markers.
    while let Some(last) = lines.last().cloned() {
        let trimmed = last.trim();
        if trimmed.is_empty() {
            lines.pop();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Added in ") {
            added_in = Some(rest.trim_end_matches('.').to_string());
            lines.pop();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Default: ") {
            default = Some(rest.trim_end_matches('.').to_string());
            lines.pop();
            continue;
        }
        if trimmed == "Opaque." {
            opaque = true;
            lines.pop();
            continue;
        }
        break;
    }
    ParsedMeta {
        body: lines.join("\n").trim().to_string(),
        added_in,
        default,
        opaque,
    }
}

fn render_section(s: &OptionSection) -> String {
    let mut out = format!("## {}\n\n", s.path);
    if !s.description.is_empty() {
        out.push_str(&s.description);
        out.push_str("\n\n");
    }
    let mut meta = vec![format!("*Type:* {}", s.type_label)];
    if let Some(default) = &s.default {
        meta.push(format!("*Default:* {}", default));
    }
    out.push_str(&meta.join(" · "));
    out.push('\n');
    if let Some(version) = &s.added_in {
        out.push_str(&format!("\n!!! tip \"New in version {}\"\n", version));
    }
    out.push('\n');
    out
}

fn description_of(schema: &serde_json::Value) -> Option<String> {
    schema
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn resolve_ref(
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    // Direct $ref
    if let Some(reference) = schema.get("$ref").and_then(|v| v.as_str())
        && let Some(name) = reference.strip_prefix("#/$defs/")
        && let Some(target) = defs.get(name)
    {
        return target.clone();
    }
    // anyOf with one $ref + null -> resolve the $ref.
    if let Some(any) = schema.get("anyOf").and_then(|v| v.as_array()) {
        let non_null: Vec<&serde_json::Value> = any
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
            .collect();
        if non_null.len() == 1 {
            return resolve_ref(non_null[0], defs);
        }
    }
    schema.clone()
}

/// Returns a markdown-ready type expression including outer backticks.
fn type_label(
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> String {
    format!("`{}`", type_label_inner(schema, defs))
}

fn type_label_inner(
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let ref_name = ref_target(schema);
    let resolved = resolve_ref(schema, defs);

    if let Some(enum_values) = resolved.get("enum").and_then(|v| v.as_array()) {
        let values: Vec<String> = enum_values
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        return values.join(" | ");
    }

    if let Some(any) = resolved.get("anyOf").and_then(|v| v.as_array()) {
        let parts: Vec<String> = any
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))
            .map(|v| type_label_inner(v, defs))
            .collect();
        if !parts.is_empty() {
            return parts.join(" | ");
        }
    }

    if let Some(types) = resolved.get("type").and_then(|v| v.as_array()) {
        let non_null: Vec<&str> = types
            .iter()
            .filter_map(|t| t.as_str())
            .filter(|s| *s != "null")
            .collect();
        if non_null.len() == 1 {
            return scalar_label_inner(non_null[0], &resolved, defs, ref_name.as_deref());
        }
    }

    if let Some(ty) = resolved.get("type").and_then(|v| v.as_str()) {
        return scalar_label_inner(ty, &resolved, defs, ref_name.as_deref());
    }

    ref_name.unwrap_or_else(|| "unknown".to_string())
}

fn scalar_label_inner(
    ty: &str,
    schema: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
    ref_name: Option<&str>,
) -> String {
    match ty {
        "boolean" => "boolean".to_string(),
        "string" => "string".to_string(),
        "integer" => "integer".to_string(),
        "number" => "number".to_string(),
        "array" => {
            let item_label = schema
                .get("items")
                .map(|i| type_label_inner(i, defs))
                .unwrap_or_else(|| "any".to_string());
            format!("list of {}", item_label)
        }
        "object" => {
            if let Some(additional) = schema.get("additionalProperties") {
                let inner_ref = ref_target(additional);
                let inner = inner_ref
                    .map(|n| humanize_ref_name(&n))
                    .unwrap_or_else(|| type_label_inner(additional, defs));
                format!("attribute set of {}", inner)
            } else if let Some(name) = ref_name {
                humanize_ref_name(name)
            } else {
                "attribute set".to_string()
            }
        }
        other => other.to_string(),
    }
}

/// `NixpkgsConfig` -> `nixpkgs config`, `Input` -> `input`.
fn humanize_ref_name(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        for low in ch.to_lowercase() {
            out.push(low);
        }
    }
    out
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
            // Import path doesn't exist, but root does.
            // Canonicalize the parent directory to resolve symlinks
            // (e.g. /tmp -> /run/user/...), falling back to lexical
            // normalization only when the parent doesn't exist either.
            let abs_import = if let Some(parent) = import_path.parent() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    canonical_parent.join(import_path.file_name().unwrap_or_default())
                } else if import_path.is_absolute() {
                    Self::normalize_path_components(import_path)
                } else if let Ok(cwd) = std::env::current_dir() {
                    Self::normalize_path_components(&cwd.join(import_path))
                } else {
                    return Ok(());
                }
            } else if import_path.is_absolute() {
                Self::normalize_path_components(import_path)
            } else {
                return Ok(());
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

    pub fn write(&self) -> Result<()> {
        let yaml = serde_yaml::to_string(&self)
            .into_diagnostic()
            .wrap_err("Failed to serialize config to YAML")?;
        std::fs::write(YAML_CONFIG, yaml)
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
}
