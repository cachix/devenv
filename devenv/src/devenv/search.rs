use std::collections::BTreeMap;

use cli_table::{Table, WithTitle};
use devenv_activity::instrument_activity;
use devenv_core::nix_backend::Options;
use miette::Result;
use serde::Deserialize;
use tokio::fs;

use super::Devenv;

#[derive(Deserialize)]
struct OptionResults(BTreeMap<String, OptionResult>);

#[derive(Deserialize)]
struct OptionResult {
    #[serde(rename = "type")]
    type_: String,
    default: Option<String>,
    description: String,
}

#[derive(Table)]
struct DevenvOptionResult {
    #[table(title = "Option")]
    name: String,
    #[table(title = "Type")]
    type_: String,
    #[table(title = "Default")]
    default: String,
    #[table(title = "Description")]
    description: String,
}

#[derive(Table)]
struct DevenvPackageResult {
    #[table(title = "Package")]
    name: String,
    #[table(title = "Version")]
    version: String,
    #[table(title = "Description")]
    description: String,
}

impl Devenv {
    #[instrument_activity("Searching options and packages")]
    pub async fn search(&self, name: &str) -> Result<String> {
        self.assemble().await?;

        // Run both searches concurrently
        let (options_results, package_results) =
            tokio::try_join!(self.search_options(name), self.search_packages(name))?;

        let results_options_count = options_results.len();
        let package_results_count = package_results.len();

        let mut output = String::new();

        if !package_results.is_empty() {
            let table_display = package_results
                .with_title()
                .table()
                .display()
                .expect("Failed to format package results");
            output.push_str(&format!("{table_display}\n"));
        }

        if !options_results.is_empty() {
            let table_display = options_results
                .with_title()
                .table()
                .display()
                .expect("Failed to format options results");
            output.push_str(&format!("{table_display}\n"));
        }

        output.push_str(&format!(
            "Found {package_results_count} packages and {results_options_count} options for '{name}'.\n"
        ));
        Ok(output)
    }

    async fn search_options(&self, name: &str) -> Result<Vec<DevenvOptionResult>> {
        let build_options = Options {
            cache_output: true,
            ..Default::default()
        };
        let options = self
            .nix
            .build(&["optionsJSON"], Some(build_options), None)
            .await?;
        let options_path = options[0]
            .join("share")
            .join("doc")
            .join("nixos")
            .join("options.json");
        let options_contents = fs::read(options_path)
            .await
            .expect("Failed to read options.json");
        let options_json: OptionResults =
            serde_json::from_slice(&options_contents).expect("Failed to parse options.json");

        let options_results = options_json
            .0
            .into_iter()
            .filter(|(key, _)| key.contains(name))
            .map(|(key, value)| DevenvOptionResult {
                name: key,
                type_: value.type_,
                default: value.default.unwrap_or_default(),
                description: value.description,
            })
            .collect::<Vec<_>>();

        Ok(options_results)
    }

    async fn search_packages(&self, name: &str) -> Result<Vec<DevenvPackageResult>> {
        let search_options = Options {
            cache_output: true,
            ..Default::default()
        };
        let search_results = self.nix.search(name, Some(search_options)).await?;
        let results = search_results
            .into_iter()
            .map(|(key, value)| DevenvPackageResult {
                name: format!("pkgs.{key}"),
                version: value.version,
                description: value.description.chars().take(80).collect::<String>(),
            })
            .collect::<Vec<_>>();

        Ok(results)
    }
}
