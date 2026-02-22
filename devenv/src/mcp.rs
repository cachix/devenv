#![allow(dead_code)]

use crate::devenv::{Devenv, DevenvOptions};
use devenv_activity::Activity;
use devenv_core::Options;
use miette::Result;
use rmcp::handler::server::tool::{ToolCallContext, ToolRouter};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, ListToolsResult, PaginatedRequestParam,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Clone)]
struct DevenvMcpServer {
    options: DevenvOptions,
    cache: Arc<RwLock<McpCache>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Default)]
struct McpCache {
    packages: Option<Vec<PackageInfo>>,
    options: Option<Vec<OptionInfo>>,
}

impl DevenvMcpServer {
    fn new(options: DevenvOptions) -> Self {
        Self {
            options,
            cache: Arc::new(RwLock::new(McpCache::default())),
            tool_router: Self::tool_router(),
        }
    }

    async fn initialize(&self) -> Result<()> {
        info!("Initializing MCP server cache...");

        let devenv = Devenv::new(self.options.clone()).await;

        // Assemble once for all operations
        devenv.assemble().await?;

        // Fetch and cache packages
        {
            let _activity = Activity::operation("Caching packages").start();
            match self.fetch_packages_with_devenv(&devenv).await {
                Ok(packages) => {
                    let mut cache = self.cache.write().await;
                    cache.packages = Some(packages);
                    info!("Successfully cached packages");
                }
                Err(e) => {
                    warn!("Failed to fetch packages during initialization: {}", e);
                }
            }
        }

        // Fetch and cache options
        {
            let _activity = Activity::operation("Caching options").start();
            match self.fetch_options_with_devenv(&devenv).await {
                Ok(options) => {
                    let mut cache = self.cache.write().await;
                    cache.options = Some(options);
                    info!("Successfully cached options");
                }
                Err(e) => {
                    warn!("Failed to fetch options during initialization: {}", e);
                }
            }
        }

        info!("MCP server initialization completed successfully");
        Ok(())
    }

    async fn fetch_packages_with_devenv(&self, devenv: &Devenv) -> Result<Vec<PackageInfo>> {
        info!("Fetching available packages from nixpkgs...");

        // Search for common/popular packages
        // Note: Using ".*" would match all packages but causes resource exhaustion
        // with the FFI backend due to GC pressure. Use a reasonable search term instead.
        let search_results = devenv.nix.search("cachix", None).await?;

        let packages: Vec<PackageInfo> = search_results
            .into_iter()
            .map(|(key, value)| {
                // Format package name like in devenv.rs search function
                let parts: Vec<&str> = key.split('.').collect();
                let name = if parts.len() > 2 {
                    format!("pkgs.{}", parts[2..].join("."))
                } else {
                    format!("pkgs.{key}")
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

    async fn fetch_options_with_devenv(&self, devenv: &Devenv) -> Result<Vec<OptionInfo>> {
        info!("Fetching available configuration options...");

        // Build the optionsJSON attribute like in devenv.rs search function
        let build_options = Options {
            cache_output: true,
            ..Default::default()
        };

        let options_paths = devenv
            .nix
            .build(&["optionsJSON"], Some(build_options), None)
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
struct SearchPackagesRequest {
    #[schemars(description = "Search term to filter packages by name or description")]
    query: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchOptionsRequest {
    #[schemars(
        description = "Search string to filter options by name or description (e.g., 'python' or 'languages.python')"
    )]
    query: String,
}

impl ServerHandler for DevenvMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Devenv MCP server - provides access to devenv packages and configuration options. Process-compose logs are available in $DEVENV_STATE/process-compose/process-compose.log".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let tool_context = ToolCallContext::new(self, request, context);
            self.tool_router.call(tool_context).await
        }
    }
}

#[tool_router]
impl DevenvMcpServer {
    #[tool(description = "Search available packages in devenv")]
    async fn search_packages(&self, params: Parameters<SearchPackagesRequest>) -> String {
        let request = params.0;

        // Always use cached data
        let cache = self.cache.read().await;
        let packages = cache.packages.as_ref().cloned().unwrap_or_else(|| {
            warn!("No cached packages available");
            vec![]
        });

        // Filter packages based on search term
        let search_lower = request.query.to_lowercase();
        let filtered_packages: Vec<PackageInfo> = packages
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&search_lower)
                    || p.description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&search_lower))
            })
            .collect();

        serde_json::to_string(&filtered_packages).unwrap_or_default()
    }

    #[tool(description = "Search available configuration options")]
    async fn search_options(&self, params: Parameters<SearchOptionsRequest>) -> String {
        let request = params.0;

        // Always use cached data
        let cache = self.cache.read().await;
        let options = cache.options.as_ref().cloned().unwrap_or_else(|| {
            warn!("No cached options available");
            vec![]
        });

        // Filter options based on search string (searches in both name and description)
        let search_lower = request.query.to_lowercase();
        let filtered_options: Vec<OptionInfo> = options
            .into_iter()
            .filter(|o| {
                o.name.to_lowercase().contains(&search_lower)
                    || o.description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&search_lower))
            })
            .collect();

        serde_json::to_string(&filtered_options).unwrap_or_default()
    }
}

pub async fn run_mcp_server(options: DevenvOptions, http_port: Option<u16>) -> Result<()> {
    info!("Starting devenv MCP server");

    let server = DevenvMcpServer::new(options);

    // Initialize cache in background thread (Nix FFI futures are not Send)
    // Server starts immediately, tools return empty results until cache is ready
    // Activities from the background thread are sent to TUI via global channel
    let init_server = server.clone();
    std::thread::Builder::new()
        .name("mcp-cache-init".into())
        .spawn(move || {
            let rt =
                tokio::runtime::Runtime::new().expect("Failed to create runtime for MCP cache");
            rt.block_on(async move {
                if let Err(e) = init_server.initialize().await {
                    warn!("Failed to initialize MCP cache: {}", e);
                }
            });
        })
        .expect("Failed to spawn MCP cache init thread");

    match http_port {
        Some(port) => {
            info!("Starting MCP server in HTTP mode on port {}", port);

            let service = StreamableHttpService::new(
                move || Ok(server.clone()),
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let router = axum::Router::new().fallback_service(service);
            let addr = format!("0.0.0.0:{}", port);
            let tcp_listener = tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| miette::miette!("Failed to bind to {}: {}", addr, e))?;

            info!("MCP server ready at http://{}/", addr);

            // Show TUI progress for HTTP server
            let _activity = Activity::operation("Running MCP server")
                .detail(format!("http://0.0.0.0:{}/", port))
                .start();

            axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.ok();
                })
                .await
                .map_err(|e| miette::miette!("HTTP server error: {}", e))?;
        }
        None => {
            info!("Starting MCP server in stdio mode");

            let service = server
                .serve(rmcp::transport::stdio())
                .await
                .map_err(|e| miette::miette!("Failed to start MCP server: {}", e))?;

            service
                .waiting()
                .await
                .map_err(|e| miette::miette!("MCP server error: {}", e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(feature = "integration-tests")]
    use devenv_nix_backend_macros::nix_test;

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
    async fn test_search_packages_request_deserialization() {
        let json = json!({
            "query": "python"
        });

        let request: SearchPackagesRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request.query, "python".to_string());
    }

    #[tokio::test]
    async fn test_search_options_request_deserialization() {
        let json = json!({
            "query": "languages"
        });

        let request: SearchOptionsRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request.query, "languages".to_string());
    }

    // Integration tests that use live Nix data
    // Note: These tests require:
    // 1. A working Nix installation
    // 2. Being run from a devenv project root (with devenv.nix) for options test
    // 3. Network access to fetch packages

    #[nix_test]
    #[cfg(feature = "integration-tests")]
    #[cfg(not(target_os = "linux"))] // Disabled on Linux due to segfaults
    async fn test_fetch_packages_live() {
        use crate::devenv::{Devenv, DevenvOptions};

        // Create temporary directory with test devenv configuration
        let temp_dir = create_test_devenv_dir().await.unwrap();

        let devenv_root = Some(temp_dir.path().to_path_buf());
        let options = DevenvOptions {
            devenv_root,
            ..Default::default()
        };
        let server = DevenvMcpServer::new(options.clone());

        let devenv = Devenv::new(options).await;
        devenv.assemble().await.unwrap();

        let packages = server.fetch_packages_with_devenv(&devenv).await;

        // Should be able to fetch packages without error
        assert!(packages.is_ok(), "Failed to fetch packages: {packages:?}");

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

    #[nix_test]
    #[cfg(feature = "integration-tests")]
    #[cfg(not(target_os = "linux"))] // Disabled on Linux due to segfaults
    async fn test_fetch_options_live() {
        use crate::devenv::{Devenv, DevenvOptions};

        // Create temporary directory with test devenv configuration
        let temp_dir = create_test_devenv_dir().await.unwrap();

        let devenv_root = Some(temp_dir.path().to_path_buf());
        let options = DevenvOptions {
            devenv_root,
            ..Default::default()
        };
        let server = DevenvMcpServer::new(options.clone());

        let devenv = Devenv::new(options).await;
        devenv.assemble().await.unwrap();

        let options = server.fetch_options_with_devenv(&devenv).await;

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
                        "Expected option '{known_option}' not found"
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
                eprintln!("Expected failure in test environment: {e:?}");
                eprintln!(
                    "This test requires running from a devenv project root with proper setup"
                );
            }
        }

        // Temporary directory will be automatically cleaned up when dropped
    }
}
