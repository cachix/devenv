use miette::{IntoDiagnostic, Result};
use schematic::{schema::JsonSchemaRenderer, schema::SchemaGenerator, ConfigLoader};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

const YAML_CONFIG: &str = "devenv.yaml";

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct FlakeInput {
    pub url: Option<String>,
    pub inputs: HashMap<String, Input>,
    pub flake: bool,
}

impl From<&Input> for FlakeInput {
    fn from(input: &Input) -> Self {
        FlakeInput {
            url: input.url.clone(),
            inputs: input.inputs.clone(),
            flake: input.flake,
        }
    }
}

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

#[derive(schematic::Config, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clean {
    pub enabled: bool,
    pub keep: Vec<String>,
    // TODO: executables?
}

#[derive(schematic::Config, Clone, Serialize, Debug)]
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
    #[serde(skip_serializing_if = "Option::is_none", default = "std::option::None")]
    #[setting(nested)]
    pub clean: Option<Clean>,
    #[serde(skip_serializing_if = "is_false", default = "false_default")]
    pub impure: bool,
}

pub fn write_json_schema() {
    let mut generator = SchemaGenerator::default();
    generator.add::<Config>();
    generator
        .generate("devenv.schema.json", JsonSchemaRenderer::default())
        .expect("can't generate schema");
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut loader = ConfigLoader::<Config>::new();
        let file = Path::new(YAML_CONFIG);
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
