
use serde::{Deserialize, Serialize};
use strum_macros::{EnumString, EnumVariantNames};

#[derive(Debug, Serialize, Deserialize, EnumString, EnumVariantNames)]
#[serde(rename_all = "lowercase")]
enum InputType {
    Git,
    Github,
    Gitlab,
    Indirect,
    Sourcehut,
}

#[derive(Debug, Serialize, Deserialize)]
struct LockedInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    last_modified: u64,
    nar_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#ref: Option<String>,
    rev: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rev_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shallow: Option<bool>,
    #[serde(rename = "type")]
    input_type: InputType,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OriginalInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#ref: Option<String>,
    r#type: InputType,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Input {
    #[serde(skip_serializing_if = "Option::is_none")]
    inputs: Option<std::collections::HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locked: Option<LockedInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<OriginalInput>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Lock {
    #[serde(skip_serializing_if = "Option::is_none")]
    nodes: Option<std::collections::HashMap<String, Input>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    version: u8,
}

impl Lock {
    pub fn read(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file = std::fs::File::open(filename)?;
        let reader = std::io::BufReader::new(file);
        let lock: Lock = serde_json::from_reader(reader)?;
        Ok(lock)
    }

    pub fn write(&self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        let file = std::fs::File::create(filename)?;
        let writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    pub fn update(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // parse inputs recursively
        // generate a new lock
        Ok(())
    }
}
