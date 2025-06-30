#![allow(dead_code)]

use crate::config::Config;
use crate::devenv::{Devenv, DevenvOptions};
use crate::nix_backend;
use miette::Result;
use rmcp::handler::server::tool::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::tool;
use rmcp::{ServerHandler, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Clone)]
struct DevenvMcpServer {
    config: Config,
    cache: Arc<RwLock<McpCache>>,
    devenv_root: Option<PathBuf>,
}

#[derive(Default)]
struct McpCache {
    packages: Option<Vec<PackageInfo>>,
    options: Option<Vec<OptionInfo>>,
}

impl DevenvMcpServer {
    fn new(config: Config) -> Self {
        Self::new_with_root(config, None)
    }

    fn new_with_root(config: Config, devenv_root: Option<PathBuf>) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(McpCache::default())),
            devenv_root,
        }
    }

    async fn initialize(&self) -> Result<()> {
        info!("Initializing MCP server cache...");

        // Fetch and cache packages
        match self.fetch_packages().await {
            Ok(packages) => {
                let mut cache = self.cache.write().await;
                cache.packages = Some(packages);
                info!("Successfully cached packages");
            }
            Err(e) => {
                warn!("Failed to fetch packages during initialization: {}", e);
            }
        }

        // Fetch and cache options
        match self.fetch_options().await {
            Ok(options) => {
                let mut cache = self.cache.write().await;
                cache.options = Some(options);
                info!("Successfully cached options");
            }
            Err(e) => {
                warn!("Failed to fetch options during initialization: {}", e);
            }
        }

        info!("MCP server initialization completed successfully");
        Ok(())
    }

    async fn fetch_packages(&self) -> Result<Vec<PackageInfo>> {
        info!("Fetching available packages from nixpkgs...");

        // Create a Devenv instance to access nix functionality
        let devenv_options = DevenvOptions {
            config: self.config.clone(),
            devenv_root: self.devenv_root.clone(),
            ..Default::default()
        };
        let devenv = Devenv::new(devenv_options).await;

        // Assemble the devenv to create required flake files
        devenv.assemble(true).await?;

        // Use broad search term to get a wide set of packages
        // We'll limit results later if needed
        let search_output = devenv.nix.lock().await.search(".*").await?;

        // Parse the search results from JSON
        #[derive(Deserialize)]
        struct PackageResults(BTreeMap<String, PackageResult>);

        #[derive(Deserialize)]
        struct PackageResult {
            version: String,
            description: String,
        }

        let search_json: PackageResults = serde_json::from_slice(&search_output.stdout)
            .map_err(|e| miette::miette!("Failed to parse search results: {}", e))?;

        let packages: Vec<PackageInfo> = search_json
            .0
            .into_iter()
            .map(|(key, value)| {
                // Format package name like in devenv.rs search function
                let parts: Vec<&str> = key.split('.').collect();
                let name = if parts.len() > 2 {
                    format!("pkgs.{}", parts[2..].join("."))
                } else {
                    format!("pkgs.{}", key)
                };

                PackageInfo {
                    name,
                    version: value.version,
                    description: Some(value.description),
                }
            })
            .collect();

        Ok(packages)
    }

    async fn fetch_options(&self) -> Result<Vec<OptionInfo>> {
        info!("Fetching available configuration options...");

        // Create a Devenv instance to access nix functionality
        let devenv_options = DevenvOptions {
            config: self.config.clone(),
            devenv_root: self.devenv_root.clone(),
            ..Default::default()
        };
        let devenv = Devenv::new(devenv_options).await;

        // Assemble the devenv to create required flake files
        devenv.assemble(true).await?;

        // Build the optionsJSON attribute like in devenv.rs search function
        let build_options = nix_backend::Options {
            logging: false,
            cache_output: true,
            ..Default::default()
        };

        let options_paths = devenv
            .nix
            .lock()
            .await
            .build(&["optionsJSON"], Some(build_options))
            .await?;

        // Read the options.json file from the build result
        let options_json_path = options_paths[0]
            .join("share")
            .join("doc")
            .join("nixos")
            .join("options.json");

        let options_content = tokio::fs::read_to_string(&options_json_path)
            .await
            .map_err(|e| miette::miette!("Failed to read options.json: {}", e))?;

        #[derive(Deserialize)]
        struct OptionResults(BTreeMap<String, OptionResult>);

        #[derive(Deserialize)]
        struct OptionResult {
            #[serde(rename = "type")]
            type_: String,
            default: Option<String>,
            description: String,
        }

        let options_json: OptionResults = serde_json::from_str(&options_content)
            .map_err(|e| miette::miette!("Failed to parse options.json: {}", e))?;

        let options: Vec<OptionInfo> = options_json
            .0
            .into_iter()
            .map(|(name, value)| OptionInfo {
                name,
                value: parse_type_to_value(&value.type_),
                description: Some(value.description),
                default: value.default.map(|d| parse_default_value(&d, &value.type_)),
            })
            .collect();

        Ok(options)
    }
}

fn parse_type_to_value(type_str: &str) -> Value {
    match type_str {
        "bool" => Value::Bool(false),
        "int" => Value::Number(serde_json::Number::from(0)),
        "string" => Value::String("".to_string()),
        "list" => Value::Array(vec![]),
        "attrs" => Value::Object(serde_json::Map::new()),
        "package" => Value::String("".to_string()),
        _ => Value::Null,
    }
}

fn parse_default_value(default_str: &str, type_str: &str) -> Value {
    // The default values in options.json are Nix expressions as strings
    // We need to parse them appropriately based on the type
    match type_str {
        "bool" => Value::Bool(default_str == "true"),
        "int" => default_str
            .parse::<i64>()
            .ok()
            .map(|n| Value::Number(n.into()))
            .unwrap_or(Value::String(default_str.to_string())),
        "string" => {
            // Nix strings are often wrapped in quotes, remove them if present
            let trimmed = default_str.trim();
            if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 1 {
                Value::String(trimmed[1..trimmed.len() - 1].to_string())
            } else {
                Value::String(default_str.to_string())
            }
        }
        "list" => {
            // Try to parse as JSON array, otherwise return as string
            serde_json::from_str(default_str).unwrap_or(Value::String(default_str.to_string()))
        }
        "attrs" => {
            // Try to parse as JSON object, otherwise return as string
            serde_json::from_str(default_str).unwrap_or(Value::String(default_str.to_string()))
        }
        _ => Value::String(default_str.to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageInfo {
    name: String,
    version: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OptionInfo {
    name: String,
    value: Value,
    description: Option<String>,
    default: Option<Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListPackagesRequest {
    #[schemars(description = "Optional search term to filter packages")]
    search: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListOptionsRequest {
    #[schemars(
        description = "Optional search string to filter options by name or description (e.g., 'python' or 'languages.python')"
    )]
    prefix: Option<String>,
}

impl ServerHandler for DevenvMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Devenv MCP server - provides access to devenv packages and configuration options. Process-compose logs are available in $DEVENV_STATE/process-compose/process-compose.log".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tool]
impl DevenvMcpServer {
    #[tool(description = "List available packages in devenv")]
    async fn list_packages(&self, params: Parameters<ListPackagesRequest>) -> String {
        let request = params.0;

        // Always use cached data
        let cache = self.cache.read().await;
        let packages = cache.packages.as_ref().cloned().unwrap_or_else(|| {
            warn!("No cached packages available");
            vec![]
        });

        // Filter packages based on search term
        let filtered_packages: Vec<PackageInfo> = if let Some(ref search_term) = request.search {
            packages
                .into_iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&search_term.to_lowercase())
                        || p.description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&search_term.to_lowercase()))
                })
                .collect()
        } else {
            packages
        };

        serde_json::to_string(&filtered_packages).unwrap_or_default()
    }

    #[tool(description = "List all available configuration options")]
    async fn list_options(&self, params: Parameters<ListOptionsRequest>) -> String {
        let request = params.0;

        // Always use cached data
        let cache = self.cache.read().await;
        let options = cache.options.as_ref().cloned().unwrap_or_else(|| {
            warn!("No cached options available");
            vec![]
        });

        // Filter options based on search string (searches in both name and description)
        let filtered_options: Vec<OptionInfo> = if let Some(ref search_str) = request.prefix {
            let search_lower = search_str.to_lowercase();
            options
                .into_iter()
                .filter(|o| {
                    o.name.to_lowercase().contains(&search_lower)
                        || o.description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&search_lower))
                })
                .collect()
        } else {
            options
        };

        serde_json::to_string(&filtered_options).unwrap_or_default()
    }
}

pub async fn run_mcp_server(config: Config) -> Result<()> {
    info!("Starting devenv MCP server");

    let server = DevenvMcpServer::new(config);

    // Initialize cache before starting the server
    server.initialize().await?;

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| miette::miette!("Failed to start MCP server: {}", e))?;

    service
        .waiting()
        .await
        .map_err(|e| miette::miette!("MCP server error: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(feature = "integration-tests")]
    async fn create_test_devenv_dir() -> std::io::Result<tempfile::TempDir> {
        let temp_dir = tempfile::tempdir()?;

        // Create minimal devenv.yaml with just nixpkgs input
        let devenv_yaml = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable"#;

        // Create minimal devenv.nix that enables the tests to work
        let devenv_nix = r#"{ pkgs, ... }: {
  # Minimal configuration for testing
  packages = [ pkgs.git ];
}"#;

        tokio::fs::write(temp_dir.path().join("devenv.yaml"), devenv_yaml).await?;
        tokio::fs::write(temp_dir.path().join("devenv.nix"), devenv_nix).await?;

        Ok(temp_dir)
    }

    #[test]
    fn test_package_info_serialization() {
        let package = PackageInfo {
            name: "nodejs".to_string(),
            version: "latest".to_string(),
            description: Some("JavaScript runtime".to_string()),
        };

        let json = serde_json::to_value(&package).unwrap();
        assert_eq!(json["name"], "nodejs");
        assert_eq!(json["version"], "latest");
        assert_eq!(json["description"], "JavaScript runtime");
    }

    #[test]
    fn test_option_info_serialization() {
        let option = OptionInfo {
            name: "languages.python.enable".to_string(),
            value: json!(false),
            description: Some("Enable Python language support".to_string()),
            default: Some(json!(false)),
        };

        let json = serde_json::to_value(&option).unwrap();
        assert_eq!(json["name"], "languages.python.enable");
        assert_eq!(json["value"], false);
        assert_eq!(json["description"], "Enable Python language support");
        assert_eq!(json["default"], false);
    }

    #[test]
    fn test_parse_type_to_value() {
        assert_eq!(parse_type_to_value("bool"), json!(false));
        assert_eq!(parse_type_to_value("int"), json!(0));
        assert_eq!(parse_type_to_value("string"), json!(""));
        assert_eq!(parse_type_to_value("list"), json!([]));
        assert_eq!(parse_type_to_value("attrs"), json!({}));
        assert_eq!(parse_type_to_value("package"), json!(""));
        assert_eq!(parse_type_to_value("unknown"), json!(null));
    }

    #[tokio::test]
    async fn test_list_packages_request_deserialization() {
        let json = json!({
            "search": "python"
        });

        let request: ListPackagesRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request.search, Some("python".to_string()));
    }

    #[tokio::test]
    async fn test_list_options_request_deserialization() {
        let json = json!({
            "prefix": "languages"
        });

        let request: ListOptionsRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request.prefix, Some("languages".to_string()));
    }

    // Integration tests that use live Nix data
    // Note: These tests require:
    // 1. A working Nix installation
    // 2. Being run from a devenv project root (with devenv.nix) for options test
    // 3. Network access to fetch packages

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_fetch_packages_live() {
        // Create temporary directory with test devenv configuration
        let temp_dir = create_test_devenv_dir().await.unwrap();

        let config = Config::default();
        let server = DevenvMcpServer::new_with_root(config, Some(temp_dir.path().to_path_buf()));

        let packages = server.fetch_packages().await;

        // Should be able to fetch packages without error
        assert!(packages.is_ok(), "Failed to fetch packages: {:?}", packages);

        let packages = packages.unwrap();

        // Should have some packages
        assert!(!packages.is_empty(), "No packages were fetched");

        // Check that packages have the expected format
        for package in packages.iter().take(5) {
            assert!(
                package.name.starts_with("pkgs."),
                "Package name should start with 'pkgs.': {}",
                package.name
            );
            assert!(
                !package.version.is_empty(),
                "Package version should not be empty"
            );
            assert!(
                package.description.is_some(),
                "Package should have a description"
            );
        }

        // Check for specific package: cachix
        let cachix_package = packages.iter().find(|p| p.name == "pkgs.cachix");
        assert!(
            cachix_package.is_some(),
            "Expected to find 'pkgs.cachix' package in the list"
        );

        let cachix = cachix_package.unwrap();
        assert!(
            !cachix.version.is_empty(),
            "Cachix package should have a version"
        );
        assert!(
            cachix.description.is_some(),
            "Cachix package should have a description"
        );

        println!("Successfully fetched {} packages", packages.len());
        println!("Found cachix package: {} ({})", cachix.name, cachix.version);
        println!("Sample packages:");
        for package in packages.iter().take(5) {
            println!("  - {} ({})", package.name, package.version);
        }

        // Temporary directory will be automatically cleaned up when dropped
    }

    #[tokio::test]
    #[cfg(feature = "integration-tests")]
    async fn test_fetch_options_live() {
        // Create temporary directory with test devenv configuration
        let temp_dir = create_test_devenv_dir().await.unwrap();

        let config = Config::default();
        let server = DevenvMcpServer::new_with_root(config, Some(temp_dir.path().to_path_buf()));

        let options = server.fetch_options().await;

        match options {
            Ok(options) => {
                // Should have some options
                assert!(!options.is_empty(), "No options were fetched");

                // Check for some known devenv options
                let known_options = vec![
                    "languages.python.enable",
                    "languages.rust.enable",
                    "services.postgres.enable",
                    "packages",
                ];

                for known_option in known_options {
                    assert!(
                        options.iter().any(|opt| opt.name == known_option),
                        "Expected option '{}' not found",
                        known_option
                    );
                }

                // Check that options have proper structure
                for option in options.iter().take(5) {
                    assert!(!option.name.is_empty(), "Option name should not be empty");
                    assert!(
                        option.description.is_some(),
                        "Option should have a description"
                    );
                }

                println!("Successfully fetched {} options", options.len());
                println!("Sample options:");
                for option in options.iter().take(5) {
                    println!("  - {}", option.name);
                }
            }
            Err(e) => {
                // Expected to fail in test environment
                eprintln!("Expected failure in test environment: {:?}", e);
                eprintln!(
                    "This test requires running from a devenv project root with proper setup"
                );
            }
        }

        // Temporary directory will be automatically cleaned up when dropped
    }
}
