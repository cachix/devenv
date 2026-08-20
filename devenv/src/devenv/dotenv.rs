use super::format_shell_exports;
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const CLI_OWNED_NAMES: &[&str] = &["SHELL", "DEVENV_CMDLINE"];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DotenvConfig {
    pub enable: bool,
    pub filename: DotenvFilenames,
    #[serde(default)]
    pub substitution: bool,
    #[serde(default)]
    pub reserved_names: BTreeSet<String>,
    pub disable_hint: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum DotenvFilenames {
    One(String),
    Many(Vec<String>),
}

impl DotenvFilenames {
    fn iter(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::One(path) => Box::new(std::iter::once(path.as_str())),
            Self::Many(paths) => Box::new(paths.iter().map(String::as_str)),
        }
    }
}

impl DotenvConfig {
    pub fn paths(&self, root: &Path) -> Vec<PathBuf> {
        self.filename
            .iter()
            .map(|path| {
                let path = Path::new(path);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    root.join(path)
                }
            })
            .collect()
    }

    pub fn watch_paths(&self, root: &Path) -> Vec<PathBuf> {
        if self.enable || !self.disable_hint {
            self.paths(root)
        } else {
            Vec::new()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DotenvEnvironment {
    variables: BTreeMap<String, String>,
    messages: Vec<String>,
}

impl DotenvEnvironment {
    pub fn load(root: &Path, config: &DotenvConfig) -> Result<Self> {
        let paths = config.paths(root);

        if !config.enable {
            let messages = if !config.disable_hint && paths.iter().any(|path| path.is_file()) {
                vec![
                    "💡 A dotenv file was found, while dotenv integration is currently not enabled."
                        .to_string(),
                    String::new(),
                    "   To enable it, add `dotenv.enable = true;` to your devenv.nix file."
                        .to_string(),
                    "   To disable this hint, add `dotenv.disableHint = true;` to your devenv.nix file."
                        .to_string(),
                    String::new(),
                    "See https://devenv.sh/integrations/dotenv/ for more information."
                        .to_string(),
                ]
            } else {
                Vec::new()
            };
            return Ok(Self {
                messages,
                ..Self::default()
            });
        }

        let mut messages = Vec::new();
        for path in paths.iter().filter(|path| !path.exists()) {
            messages.push(format!(
                "💡 The dotenv file '{}' was not found.",
                display_path(root, path)
            ));

            let example = path_with_suffix(path, ".example");
            if example.is_file() {
                messages.extend([
                    String::new(),
                    "   To create this file, you can copy the example file:".to_string(),
                    String::new(),
                    format!(
                        "   $ cp {} {}",
                        display_path(root, &example),
                        display_path(root, path)
                    ),
                    String::new(),
                ]);
            }
        }

        // An empty filename list is a useful way for shared configuration to
        // opt out without also having to override `enable`.
        if paths.is_empty() {
            return Ok(Self {
                messages,
                ..Self::default()
            });
        }

        let mut variables = devenv_core::dotenv::load_dotenv(&paths, config.substitution)?;
        variables.retain(|name, _| {
            !config.reserved_names.contains(name) && !CLI_OWNED_NAMES.contains(&name.as_str())
        });

        Ok(Self {
            variables,
            messages,
        })
    }

    pub fn message_script(&self) -> String {
        let mut script = String::new();
        for message in &self.messages {
            script.push_str("printf '%s\\n' ");
            script.push_str(&shell_escape::escape(std::borrow::Cow::Borrowed(message)));
            script.push_str(" >&2\n");
        }
        script
    }

    /// Export runtime dotenv values after Nix activation. PATH-like variables
    /// are appended only when absent so the Nix and caller paths survive and a
    /// value already represented by the Nix environment is not duplicated.
    pub fn activation_script(&self) -> String {
        const PATH_LIKE_NAMES: &[&str] = &["PATH", "XDG_DATA_DIRS"];

        let regular_variables = self
            .variables
            .iter()
            .filter(|(name, _)| !PATH_LIKE_NAMES.contains(&name.as_str()))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let mut script = format_shell_exports(&regular_variables);

        for name in PATH_LIKE_NAMES {
            let Some(value) = self.variables.get(*name) else {
                continue;
            };
            let escaped = shell_escape::escape(std::borrow::Cow::Borrowed(value));
            script.push_str(&format!(
                "case \":${{{name}:-}}:\" in\n  *:{escaped}:*) ;;\n  *) export {name}=\"${{{name}:+${name}:}}\"{escaped} ;;\nesac\n"
            ));
        }

        script
    }

    pub fn merge_json(&self, json: &str) -> Result<String> {
        let mut value: serde_json::Value = serde_json::from_str(json)
            .into_diagnostic()
            .wrap_err("Failed to parse shell environment JSON")?;
        let variables = value
            .get_mut("variables")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| miette::miette!("Shell environment JSON has no variables object"))?;

        // Nix-defined variables have higher precedence than dotenv, matching the old
        // lib.mkDefault behavior.
        for (name, value) in &self.variables {
            variables.entry(name.clone()).or_insert_with(|| {
                serde_json::json!({
                    "type": "exported",
                    "value": value,
                })
            });
        }

        serde_json::to_string(&value)
            .into_diagnostic()
            .wrap_err("Failed to serialize shell environment JSON")
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = std::ffi::OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn config(files: Vec<String>) -> DotenvConfig {
        DotenvConfig {
            enable: true,
            filename: DotenvFilenames::Many(files),
            substitution: false,
            reserved_names: BTreeSet::new(),
            disable_hint: false,
        }
    }

    #[test]
    fn loads_full_dotenv_syntax_and_layers_files() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("config")).unwrap();
        fs::write(
            root.path().join(".env"),
            "PLAIN=one\nQUOTED=\"two words\"\nJSON={\"key\":true}\nMULTILINE='first\nsecond'\nHASH=value#literal\nCOMMENTED=value # comment\nLAYER=base\n",
        )
        .unwrap();
        fs::write(
            root.path().join("config/.env.local"),
            "LAYER=local\nEXPORTED=works\n",
        )
        .unwrap();

        let environment = DotenvEnvironment::load(
            root.path(),
            &config(vec![".env".into(), "config/.env.local".into()]),
        )
        .unwrap();

        assert_eq!(environment.variables["PLAIN"], "one");
        assert_eq!(environment.variables["QUOTED"], "two words");
        assert_eq!(environment.variables["JSON"], r#"{"key":true}"#);
        assert_eq!(environment.variables["MULTILINE"], "first\nsecond");
        assert_eq!(environment.variables["HASH"], "value#literal");
        assert_eq!(environment.variables["COMMENTED"], "value");
        assert_eq!(environment.variables["LAYER"], "local");
        assert_eq!(environment.variables["EXPORTED"], "works");
    }

    #[test]
    fn substitution_is_opt_in() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env"), "A=zero\nB=${A}+one\n").unwrap();

        let literal = DotenvEnvironment::load(root.path(), &config(vec![".env".into()])).unwrap();
        assert_eq!(literal.variables["B"], "${A}+one");

        let mut expanded_config = config(vec![".env".into()]);
        expanded_config.substitution = true;
        let expanded = DotenvEnvironment::load(root.path(), &expanded_config).unwrap();
        assert_eq!(expanded.variables["B"], "zero+one");
    }

    #[test]
    fn missing_files_are_optional_and_reported_at_runtime() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env.example"), "VALUE=example\n").unwrap();

        let environment =
            DotenvEnvironment::load(root.path(), &config(vec![".env".into()])).unwrap();

        assert!(environment.variables.is_empty());
        let messages = environment.message_script();
        assert!(messages.contains("was not found"));
        assert!(messages.contains("cp .env.example .env"));
    }

    #[test]
    fn disabled_integration_reports_an_existing_file_unless_suppressed() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env"), "VALUE=present\n").unwrap();
        let mut config = config(vec![".env".into()]);
        config.enable = false;

        let environment = DotenvEnvironment::load(root.path(), &config).unwrap();
        assert!(environment.message_script().contains("not enabled"));
        assert_eq!(
            config.watch_paths(root.path()),
            vec![root.path().join(".env")]
        );

        config.disable_hint = true;
        let environment = DotenvEnvironment::load(root.path(), &config).unwrap();
        assert!(environment.message_script().is_empty());
        assert!(config.watch_paths(root.path()).is_empty());
    }

    #[test]
    fn an_empty_filename_list_loads_nothing() {
        let root = TempDir::new().unwrap();

        let environment = DotenvEnvironment::load(root.path(), &config(Vec::new())).unwrap();

        assert!(environment.variables.is_empty());
    }

    #[test]
    fn nix_json_values_override_dotenv() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env"), "DOTENV_ONLY=yes\nSHARED=dotenv\n").unwrap();
        let environment =
            DotenvEnvironment::load(root.path(), &config(vec![".env".into()])).unwrap();
        let merged = environment
            .merge_json(
                r#"{"variables":{"SHARED":{"type":"exported","value":"nix"}},"bashFunctions":{}}"#,
            )
            .unwrap();
        let merged: serde_json::Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(merged["variables"]["SHARED"]["value"], "nix");
        assert_eq!(merged["variables"]["DOTENV_ONLY"]["value"], "yes");
    }

    #[test]
    fn nix_owned_names_are_not_loaded_from_dotenv() {
        let root = TempDir::new().unwrap();
        fs::write(
            root.path().join(".env"),
            "DOTENV_ONLY=yes\nNIX_OWNED=dotenv\n",
        )
        .unwrap();
        let mut config = config(vec![".env".into()]);
        config.reserved_names.insert("NIX_OWNED".into());

        let environment = DotenvEnvironment::load(root.path(), &config).unwrap();

        assert_eq!(environment.variables["DOTENV_ONLY"], "yes");
        assert!(!environment.variables.contains_key("NIX_OWNED"));
    }

    #[test]
    fn cli_owned_names_are_not_loaded_from_dotenv() {
        let root = TempDir::new().unwrap();
        fs::write(
            root.path().join(".env"),
            "DOTENV_ONLY=yes\nSHELL=/dotenv/shell\nDEVENV_CMDLINE=dotenv\n",
        )
        .unwrap();

        let environment =
            DotenvEnvironment::load(root.path(), &config(vec![".env".into()])).unwrap();

        assert_eq!(environment.variables["DOTENV_ONLY"], "yes");
        assert!(!environment.variables.contains_key("SHELL"));
        assert!(!environment.variables.contains_key("DEVENV_CMDLINE"));
    }

    #[test]
    fn activation_appends_path_like_values_without_replacing_existing_paths() {
        let root = TempDir::new().unwrap();
        fs::write(
            root.path().join(".env"),
            "PATH=/dotenv/bin\nXDG_DATA_DIRS=/dotenv/share\nREGULAR=value\n",
        )
        .unwrap();

        let environment =
            DotenvEnvironment::load(root.path(), &config(vec![".env".into()])).unwrap();
        let activation = environment.activation_script();

        assert!(activation.contains("export REGULAR=value\n"));
        assert!(activation.contains("export PATH=\"${PATH:+$PATH:}\"/dotenv/bin"));
        assert!(
            activation.contains(
                "export XDG_DATA_DIRS=\"${XDG_DATA_DIRS:+$XDG_DATA_DIRS:}\"/dotenv/share"
            )
        );
    }

    #[test]
    fn rejects_names_that_shells_cannot_export() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env"), "NOT-EXPORTABLE=value\n").unwrap();

        let error = DotenvEnvironment::load(root.path(), &config(vec![".env".into()]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Invalid environment variable name"));
    }
}
