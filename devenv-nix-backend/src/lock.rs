//! Pre-bootstrap lock-file helpers.
//!
//! Helpers operate on an explicit `EvalState` + `Store` + settings.
//! Project composition is handled by [`crate::project`].
//! Wrap construction + validation in [`with_lock_scope`] so the lazy
//! `«nix-internal»/derivation-internal.nix` load nests under the activity.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use devenv_activity::{Activity, ActivityLevel, instrument_activity};
use devenv_core::config::Input;
use devenv_core::nix_log_bridge::NixLogBridge;
use miette::{IntoDiagnostic, Result, WrapErr};
use nix_bindings_expr::eval_state::{EvalState, EvalStateBuilder};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings, LockFile};
use nix_bindings_store::store::Store;

use crate::anyhow_ext::AnyhowToMiette;

const LOCK_SOURCES: &str = include_str!("../bootstrap/lock-sources.nix");
const RESOLVE_INPUT_PATHS: &str = include_str!("../bootstrap/resolve-input-paths.nix");

/// Build a transient `EvalState` for lock-file work. Drop it when done.
pub fn build_eval_state(
    store: &Store,
    root: &Path,
    flake_settings: &FlakeSettings,
    refresh_fetchers: bool,
) -> Result<EvalState> {
    let root_str = root
        .to_str()
        .ok_or_else(|| miette::miette!("Root path contains invalid UTF-8"))?;
    let mut builder = EvalStateBuilder::new(store.clone())
        .to_miette()
        .wrap_err("Failed to create eval state builder")?
        .base_directory(root_str)
        .to_miette()
        .wrap_err("Failed to set base directory")?
        .flakes(flake_settings)
        .to_miette()
        .wrap_err("Failed to configure flakes")?;
    if refresh_fetchers {
        builder = builder
            .fetch_setting("tarball-ttl", "0")
            .to_miette()
            .wrap_err("Failed to refresh imported input sources")?;
    }
    builder
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
    let _eval_guard = bridge.begin_eval_with_span(activity.id(), activity.span());
    activity.with_new_scope_sync(f)
}

/// Validate (and create or update if needed) `lock_file`,
/// returning the fingerprint of the resulting lock graph.
pub fn validate_and_load(
    eval_state: &EvalState,
    store: &Store,
    fetchers_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    lock_file: &Path,
    inputs: &BTreeMap<String, Input>,
) -> Result<String> {
    crate::validate_lock_file(
        eval_state,
        fetchers_settings,
        flake_settings,
        root,
        lock_file,
        inputs,
    )
    .to_miette()?;
    fingerprint(store, fetchers_settings, lock_file)
}

/// Compute the fingerprint of `lock_file` against `store`.
pub fn fingerprint(
    store: &Store,
    fetchers_settings: &FetchersSettings,
    lock_file: &Path,
) -> Result<String> {
    let lock = crate::load_lock_file(fetchers_settings, lock_file).to_miette()?;
    crate::compute_lock_fingerprint(lock.as_ref(), fetchers_settings, store).to_miette()
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

pub(crate) fn input_paths_from_lock(
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
        "builtins.toJSON (({RESOLVE_INPUT_PATHS}) {{ src = {root_nix}; names = {names_nix}; lockFileJSON = {lock_nix}; sourceResolver = ({LOCK_SOURCES}); }})"
    );

    let value = eval_state
        .eval_from_string(&expression, "<devenv-input-imports>")
        .to_miette()
        .wrap_err("Failed to resolve imported input sources")?;
    // fetchTree can return a lazy store path. Materialize the JSON's string
    // context before opening those paths from Rust. Live local paths carry
    // no store context and stay on the filesystem.
    let json = eval_state
        .realise_string(&value, false)
        .to_miette()
        .wrap_err("Failed to read imported input sources")?
        .s;
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

/// Lock or update the requested inputs.
#[instrument_activity("Updating lock", kind = evaluate, level = DEBUG)]
pub fn update(
    eval_state: &EvalState,
    fetchers_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    lock_file: &Path,
    inputs: &BTreeMap<String, Input>,
    name: Option<&str>,
    overrides: &[String],
) -> Result<()> {
    crate::lock_inputs(
        eval_state,
        fetchers_settings,
        flake_settings,
        root,
        lock_file,
        inputs,
        name,
        overrides,
    )
    .to_miette()
}
