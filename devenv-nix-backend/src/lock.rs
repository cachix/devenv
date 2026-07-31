//! Pre-bootstrap lock-file helpers.
//!
//! Most helpers operate on an explicit `EvalState` + `Store` + settings. The
//! pre-backend [`resolve_config_imports`] entry point creates a transient store
//! so locked sources can be read before the full configuration is available.
//! Wrap construction + validation in [`with_lock_scope`] so the lazy
//! `«nix-internal»/derivation-internal.nix` load nests under the activity.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use devenv_activity::{Activity, ActivityLevel, instrument_activity};
use devenv_core::config::{Config, Input};
use devenv_core::nix_log_bridge::NixLogBridge;
use devenv_core::{NixSettings, StoreSettings};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix_bindings_expr::eval_state::{EvalState, EvalStateBuilder};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings, LockFile};
use nix_bindings_store::store::Store;

use crate::anyhow_ext::AnyhowToMiette;

const RESOLVE_INPUT_PATHS: &str = include_str!("../bootstrap/resolve-input-paths.nix");
const MAX_CONFIG_IMPORT_PASSES: usize = 100;

/// Build a transient `EvalState` for lock-file work. Drop it when done.
pub fn build_eval_state(
    store: &Store,
    root: &Path,
    flake_settings: &FlakeSettings,
) -> Result<EvalState> {
    let root_str = root
        .to_str()
        .ok_or_else(|| miette::miette!("Root path contains invalid UTF-8"))?;
    EvalStateBuilder::new(store.clone())
        .to_miette()
        .wrap_err("Failed to create eval state builder")?
        .base_directory(root_str)
        .to_miette()
        .wrap_err("Failed to set base directory")?
        .flakes(flake_settings)
        .to_miette()
        .wrap_err("Failed to configure flakes")?
        .build()
        .to_miette()
        .wrap_err("Failed to build eval state")
}

/// Run `f` inside a "Validating lock" activity + `begin_eval` scope.
///
/// Wrap any `EvalState` construction and validation here so Nix's lazy
/// `«nix-internal»` loads nest under the activity.
pub fn with_lock_scope<F, T>(bridge: &Arc<NixLogBridge>, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let activity =
        devenv_activity::start!(Activity::evaluate("Validating lock").level(ActivityLevel::Info));
    let _eval_guard = bridge.begin_eval(activity.id());
    activity.with_new_scope_sync(f)
}

/// Validate (and create or update if needed) `<root>/devenv.lock`,
/// returning the fingerprint of the resulting lock graph.
pub fn validate_and_load(
    eval_state: &EvalState,
    store: &Store,
    fetchers_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    inputs: &BTreeMap<String, Input>,
) -> Result<String> {
    crate::validate_lock_file(eval_state, fetchers_settings, flake_settings, root, inputs)
        .to_miette()?;
    fingerprint(store, fetchers_settings, root)
}

/// Compute the fingerprint of `<root>/devenv.lock` against `store`.
pub fn fingerprint(
    store: &Store,
    fetchers_settings: &FetchersSettings,
    root: &Path,
) -> Result<String> {
    let lock_file_path = root.join("devenv.lock");
    let lock_file = crate::load_lock_file(fetchers_settings, &lock_file_path).to_miette()?;
    crate::compute_lock_fingerprint(lock_file.as_ref(), fetchers_settings, store).to_miette()
}

/// Materialize selected top-level lock inputs and return their source roots.
///
/// The resolver understands follows and `dir` references and mirrors the live
/// path-input behavior used by `bootstrap/resolve-lock.nix`.
pub fn input_paths(
    eval_state: &mut EvalState,
    root: &Path,
    names: &BTreeSet<String>,
) -> Result<BTreeMap<String, PathBuf>> {
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }
    let lock_path = root.join("devenv.lock");
    let lock_json = std::fs::read_to_string(&lock_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read {}", lock_path.display()))?;
    input_paths_from_json(eval_state, root, names, &lock_json)
}

fn input_paths_from_lock(
    eval_state: &mut EvalState,
    root: &Path,
    names: &BTreeSet<String>,
    lock_file: &LockFile,
) -> Result<BTreeMap<String, PathBuf>> {
    let lock_json = lock_file
        .to_string()
        .to_miette()
        .wrap_err("Failed to serialize composed lock file")?;
    input_paths_from_json(eval_state, root, names, &lock_json)
}

fn input_paths_from_json(
    eval_state: &mut EvalState,
    root: &Path,
    names: &BTreeSet<String>,
    lock_json: &str,
) -> Result<BTreeMap<String, PathBuf>> {
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }

    let root = root.canonicalize().into_diagnostic().wrap_err_with(|| {
        format!(
            "Failed to canonicalize project root while resolving imports: {}",
            root.display()
        )
    })?;
    let root_nix = ser_nix::to_string(&root.to_string_lossy().as_ref())
        .into_diagnostic()
        .wrap_err("Failed to serialize project root for Nix")?;
    let names_nix = ser_nix::to_string(&names.iter().collect::<Vec<_>>())
        .into_diagnostic()
        .wrap_err("Failed to serialize imported input names for Nix")?;
    let lock_nix = ser_nix::to_string(&lock_json)
        .into_diagnostic()
        .wrap_err("Failed to serialize composed lock file for Nix")?;
    let expression = format!(
        "builtins.toJSON (({RESOLVE_INPUT_PATHS}) {{ src = {root_nix}; names = {names_nix}; lockFileJSON = {lock_nix}; }})"
    );

    let value = eval_state
        .eval_from_string(&expression, "<devenv-input-imports>")
        .to_miette()
        .wrap_err("Failed to resolve imported input sources")?;
    let json = eval_state
        .require_string(&value)
        .to_miette()
        .wrap_err("Failed to read imported input sources")?;
    let paths: BTreeMap<String, PathBuf> = serde_json::from_str(&json)
        .into_diagnostic()
        .wrap_err("Failed to parse imported input sources")?;

    paths
        .into_iter()
        .map(|(name, path)| {
            let path = path.canonicalize().into_diagnostic().wrap_err_with(|| {
                format!(
                    "Failed to canonicalize source for imported input '{name}': {}",
                    path.display()
                )
            })?;
            Ok((name, path))
        })
        .collect()
}

/// Resolve input-style YAML imports to a fixed point.
///
/// `reload` reparses the project with the locked input source map and reapplies
/// any caller-owned overlays (for example CLI input overrides). The root lock
/// file remains the single lock graph: imported `devenv.lock` files are not
/// merged, so existing config precedence also determines input precedence.
pub fn resolve_config_imports<F>(
    root: &Path,
    mut config: Config,
    nix_settings: &NixSettings,
    mut reload: F,
) -> Result<Config>
where
    F: FnMut(&BTreeMap<String, PathBuf>) -> Result<Config>,
{
    if config.input_import_names().is_empty() {
        return Ok(config);
    }

    let _gc_registration = crate::backend::init_nix(nix_settings, &StoreSettings::default())?;
    let store = crate::backend::open_store(&StoreSettings::default())?;
    let (flake_settings, fetchers_settings) = crate::backend::build_settings()?;
    let mut eval_state = build_eval_state(&store, root, &flake_settings)?;
    let old_lock = crate::load_lock_file(&fetchers_settings, &root.join("devenv.lock"))
        .to_miette()
        .wrap_err("Failed to load lock file while resolving remote YAML imports")?;

    for _ in 0..MAX_CONFIG_IMPORT_PASSES {
        // Build each candidate from the on-disk graph so pins belonging only
        // to imported YAML survive the initial, root-only config pass. Nothing
        // is written until the complete import graph has stabilized.
        let resolved_lock = crate::resolve_lock_file(
            &eval_state,
            &fetchers_settings,
            &flake_settings,
            root,
            &config.inputs,
            old_lock.as_ref(),
        )
        .to_miette()
        .wrap_err("Failed to resolve inputs needed by remote YAML imports")?;

        let sources = input_paths_from_lock(
            &mut eval_state,
            root,
            &config.input_import_names(),
            &resolved_lock,
        )?;
        let next = reload(&sources)?;
        let import_graph_is_stable = next.inputs == config.inputs && next.imports == config.imports;
        if import_graph_is_stable {
            crate::write_lock_file(&resolved_lock, &root.join("devenv.lock"))
                .to_miette()
                .wrap_err("Failed to write lock file after composing remote YAML imports")?;
            return Ok(next);
        }
        config = next;
    }

    bail!(
        "Remote devenv.yaml imports did not stabilize after {} passes. Check for imports or inputs that change each other recursively.",
        MAX_CONFIG_IMPORT_PASSES
    )
}

/// Lock or update the requested inputs.
#[instrument_activity("Updating lock", kind = evaluate, level = DEBUG)]
pub fn update(
    eval_state: &EvalState,
    fetchers_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    inputs: &BTreeMap<String, Input>,
    name: Option<&str>,
    overrides: &[String],
) -> Result<()> {
    crate::lock_inputs(
        eval_state,
        fetchers_settings,
        flake_settings,
        root,
        inputs,
        name,
        overrides,
    )
    .to_miette()
}
