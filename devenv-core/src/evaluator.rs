//! Generic Nix evaluator interface.
//!
//! "A thing that can talk to Nix": evaluate attribute paths against the
//! project's devenv config root to JSON, build a derivation to store
//! paths, and surface a [`Store`] for the consumer. Nothing devenv-shaped
//! beyond the bootstrap-args contract (which the evaluator owns at
//! construction time, opaque-payload style). Anything else lives on
//! [`crate::Backend`].

use std::path::PathBuf;

use async_trait::async_trait;
use miette::Result;
use serde::{Deserialize, Serialize};

use crate::store::{Store, StorePath};

/// Options for [`Evaluator::build`].
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// Optional GC root directory; if set, every output path gets a
    /// permanent GC root under this directory (named after the attr).
    pub gc_root: Option<PathBuf>,
}

/// Devenv shell environment build output.
#[derive(Debug, Clone, Default)]
pub struct DevEnvOutput {
    /// The bash environment script.
    pub bash_env: Vec<u8>,
    /// File paths that the evaluation depends on (for direnv to watch).
    pub inputs: Vec<PathBuf>,
}

/// Package search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchResult {
    pub pname: String,
    pub version: String,
    pub description: String,
}

/// Map of attr-path → package search result.
pub type SearchResults = std::collections::BTreeMap<String, PackageSearchResult>;

/// Formatted flake metadata (lock file inputs + `config.info`).
pub type NixMetadata = String;

/// "A thing that can talk to Nix."
///
/// `eval` and `build` take attribute paths into the project's devenv
/// config root. The evaluator owns the bootstrap-args wiring (passed at
/// construction time) and the primop binding (set via the inherent
/// `set_port_allocator` on the concrete backend).
#[async_trait(?Send)]
pub trait Evaluator: Send + Sync {
    /// Backend name (for logging / debugging).
    fn name(&self) -> &str;

    /// Store this evaluator is bound to.
    fn store(&self) -> &dyn Store;

    /// Evaluate `attrs` as an attribute path against the devenv config
    /// root and return JSON.
    async fn eval(&self, attrs: &[&str]) -> Result<String>;

    /// Build the derivation at `attrs` and return its output paths.
    async fn build(&self, attrs: &[&str], opts: BuildOptions) -> Result<Vec<StorePath>>;

    /// Object-safe downcast hook for callers that need the concrete
    /// backend type for evaluator-specific operations (lock updates,
    /// REPL, native dev-env, search). Implementors return `self`.
    fn as_any(&self) -> &dyn std::any::Any;
}
