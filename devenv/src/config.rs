use miette::{IntoDiagnostic, Result};
use schemars::{schema_for, JsonSchema};
use schematic::ConfigLoader;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, path::Path};

const YAML_CONFIG: &str = "devenv.yaml";

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
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub inputs: HashMap<String, Input>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub overlays: Vec<String>,
}

impl Input {
    pub fn new() -> Self {
        Input {
            url: None,
            flake: true,
            follows: None,
            inputs: HashMap::new(),
            overlays: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct FlakeInput {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub follows: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub inputs: HashMap<String, Input>,
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
#[config(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    #[setting(nested)]
    pub inputs: HashMap<String, Input>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub allow_unfree: bool,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub allow_broken: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub imports: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub permitted_insecure_packages: Vec<String>,
    #[setting(nested)]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub clean: Option<Clean>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub impure: bool,
}

// TODO: https://github.com/moonrepo/schematic/issues/105
pub fn write_json_schema() {
    let schema = schema_for!(Config);
    let schema = serde_json::to_string_pretty(&schema).unwrap();
    let path = Path::new("docs/devenv.schema.json");
    std::fs::write(path, schema)
        .unwrap_or_else(|_| panic!("Failed to write json schema to {}", path.display()));
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

    pub fn write(&self) {
        let yaml = serde_yaml::to_string(&self).unwrap();
        std::fs::write(YAML_CONFIG, yaml).expect("Failed to write devenv.yaml");
    }

    pub fn add_input(&mut self, name: &str, url: &str, follows: &[String]) {
        let mut inputs = HashMap::new();

        let mut input_names = self.inputs.clone();
        // we know we have a default for this one
        input_names.insert(String::from("nixpkgs"), Input::new());

        for follow in follows {
            // check if it's not in self.inputs
            match input_names.get(follow) {
                Some(_) => {
                    let mut input = Input::new();
                    input.follows = Some(follow.to_string());
                    inputs.insert(follow.to_string(), input);
                }
                None => {
                    panic!("Input {follow} does not exist so it can't be followed.");
                }
            }
        }
        let mut input = Input::new();
        input.url = Some(url.to_string());
        input.inputs = inputs;
        self.inputs.insert(name.to_string(), input);
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
}
