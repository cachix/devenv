use miette::{IntoDiagnostic, Result, WrapErr};
use schemars::{schema_for, JsonSchema};
use schematic::ConfigLoader;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, path::Path};

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
    pub allow_unfree: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub allow_broken: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub cuda_support: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cuda_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
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
    pub config_: NixpkgsConfig,
    #[serde(
        rename = "per-platform",
        skip_serializing_if = "BTreeMap::is_empty",
        default
    )]
    pub per_platform: BTreeMap<String, NixpkgsConfig>,
}

#[derive(schematic::Config, Clone, Serialize, Debug, JsonSchema)]
#[config(rename_all = "camelCase", allow_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    #[setting(nested)]
    pub inputs: BTreeMap<String, Input>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub allow_unfree: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub allow_broken: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[setting(nested)]
    pub nixpkgs: Option<Nixpkgs>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub imports: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub permitted_insecure_packages: Vec<String>,
    #[setting(nested)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub clean: Option<Clean>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub impure: bool,
    #[serde(default)]
    pub backend: NixBackendType,
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

    pub fn load_from<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let file = path.as_ref().join(YAML_CONFIG);
        let mut loader = ConfigLoader::<Config>::new();
        let _ = loader.file_optional(file);
        let result = loader.load().into_diagnostic();
        Ok(result?.config)
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
}
