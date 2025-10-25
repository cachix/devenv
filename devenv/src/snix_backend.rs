//! Snix backend implementation for devenv.
//!
//! This module provides a Rust-native Nix evaluator backend using Snix
//! as an alternative to the traditional C++ Nix binary.

use crate::nix_backend::{DevenvPaths, NixBackend, Options};
use crate::{cli, config};
use async_trait::async_trait;
use devenv_eval_cache::Output;
use miette::{Result, bail};
use snix_build::buildservice::{BuildService, DummyBuildService};
use snix_castore::blobservice::from_addr as blob_from_addr;
use snix_castore::directoryservice::from_addr as directory_from_addr;
use snix_glue::snix_io::SnixIO;
use snix_glue::snix_store_io::SnixStoreIO;
use snix_store::nar::{NarCalculationService, SimpleRenderer};
use snix_store::pathinfoservice::from_addr as pathinfo_from_addr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Snix backend implementation for devenv.
///
/// NOTE: The snix library (EvaluationBuilder and Evaluation types) uses single-threaded
/// reference-counted pointers (Rc<T>) internally, making these types incompatible with
/// multi-threaded sharing. This backend creates fresh evaluator instances per operation
/// rather than storing them in shared state.
pub struct SnixBackend {
    #[allow(dead_code)] // Will be used when more functionality is implemented
    config: config::Config,
    #[allow(dead_code)] // Will be used when more functionality is implemented
    global_options: cli::GlobalOptions,
    #[allow(dead_code)] // Will be used when more functionality is implemented
    paths: DevenvPaths,
}

impl SnixBackend {
    pub async fn new(
        config: config::Config,
        global_options: cli::GlobalOptions,
        paths: DevenvPaths,
    ) -> Result<Self> {
        info!("Initializing Snix backend");

        Ok(Self {
            config,
            global_options,
            paths,
        })
    }

    /// Create a fresh Snix evaluator for a single operation.
    ///
    /// Since snix types use single-threaded Rc pointers, we cannot store them in shared state.
    /// Instead, we create a fresh evaluator for each operation.
    #[allow(dead_code)] // Will be used when backend methods are fully implemented
    async fn create_evaluator(
        &self,
    ) -> Result<snix_eval::Evaluation<'static, 'static, 'static, Box<dyn snix_eval::EvalIO>>> {
        debug!("Creating fresh Snix evaluator");

        // Create the required services
        let blob_service = blob_from_addr("memory://")
            .await
            .map_err(|e| miette::miette!("Failed to create blob service: {}", e))?;
        let directory_service = directory_from_addr("memory://")
            .await
            .map_err(|e| miette::miette!("Failed to create directory service: {}", e))?;
        let path_info_service = pathinfo_from_addr(
            "memory://",
            None, // Use default composition context
        )
        .await
        .map_err(|e| miette::miette!("Failed to create path info service: {}", e))?;

        let nar_calculation_service: Arc<dyn NarCalculationService> = Arc::new(
            SimpleRenderer::new(blob_service.clone(), directory_service.clone()),
        );

        let build_service: Arc<dyn BuildService> = Arc::new(DummyBuildService {});

        // Create a Snix store I/O handle
        let io_handle = Rc::new(SnixStoreIO::new(
            blob_service,
            directory_service,
            path_info_service,
            nar_calculation_service,
            build_service,
            tokio::runtime::Handle::current(),
            vec![], // No hashed mirrors for now
        ));

        // Create evaluation builder
        let io = Box::new(SnixIO::new(io_handle.clone() as Rc<dyn snix_eval::EvalIO>))
            as Box<dyn snix_eval::EvalIO>;
        let mut eval_builder = snix_eval::Evaluation::builder(io)
            .enable_import()
            .add_builtins(snix_eval::builtins::impure_builtins());

        // Configure evaluation mode
        // Note: Snix uses Strict/Lazy modes, not Impure/Pure
        // Impure is controlled by the IO handle and builtins
        eval_builder = eval_builder.mode(snix_eval::EvalMode::Lazy);

        // Set up NIX_PATH if needed
        if let Ok(nix_path) = std::env::var("NIX_PATH") {
            eval_builder = eval_builder.nix_path(Some(nix_path));
        }

        // Build the final evaluator
        Ok(eval_builder.build())
    }
}

#[async_trait(?Send)]
impl NixBackend for SnixBackend {
    async fn assemble(&self) -> Result<()> {
        // No shared state to initialize - evaluators are created per operation
        Ok(())
    }

    async fn dev_env(&self, _json: bool, _gc_root: &Path) -> Result<Output> {
        // TODO: This is a complex operation that requires implementing the equivalent
        // of `nix print-dev-env`. For now, we'll return a placeholder error.
        bail!(
            "dev_env is not yet implemented for Snix backend. This requires implementing shell environment generation."
        )
    }

    async fn add_gc(&self, _name: &str, _path: &Path) -> Result<()> {
        // TODO: Implement GC root management for Snix
        warn!("GC root management not yet implemented for Snix backend");
        Ok(())
    }

    async fn repl(&self) -> Result<()> {
        // TODO: Implement REPL functionality
        bail!("REPL is not yet implemented for Snix backend")
    }

    async fn build(
        &self,
        _attributes: &[&str],
        _options: Option<Options>,
        _gc_root: Option<&Path>,
    ) -> Result<Vec<PathBuf>> {
        // TODO: This requires implementing the build functionality
        // using snix_glue::snix_build
        bail!("Build functionality is not yet implemented for Snix backend")
    }

    async fn eval(&self, attributes: &[&str]) -> Result<String> {
        // Convert attributes to a Nix expression
        let _expr = if attributes.is_empty() {
            "{ }".to_string()
        } else {
            // Build an attribute path expression like ".#foo.bar"
            let attr_path = attributes.join(".");
            format!("(import ./flake.nix).{attr_path}")
        };

        // For now, return a placeholder - proper implementation would need generator context
        bail!("eval() is not yet fully implemented for SnixBackend")
    }

    async fn update(&self, _input_name: &Option<String>) -> Result<()> {
        // TODO: Implement flake update functionality
        bail!("Flake update is not yet implemented for Snix backend")
    }

    async fn metadata(&self) -> Result<String> {
        // TODO: Implement flake metadata functionality
        bail!("Flake metadata is not yet implemented for Snix backend")
    }

    async fn search(&self, _name: &str, _options: Option<Options>) -> Result<Output> {
        // TODO: Implement package search functionality
        bail!("Package search is not yet implemented for Snix backend")
    }

    async fn gc(&self, _paths: Vec<PathBuf>) -> Result<()> {
        // TODO: Implement garbage collection
        warn!("Garbage collection not yet implemented for Snix backend");
        Ok(())
    }

    fn name(&self) -> &'static str {
        "snix"
    }

    async fn get_bash(&self, _refresh_cached_output: bool) -> Result<String> {
        // TODO: Implement bash shell acquisition for Snix backend
        bail!("get_bash is not yet implemented for Snix backend")
    }
}
