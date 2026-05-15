//! Pre-bootstrap lock-file helpers.
//!
//! Free functions over an explicit `EvalState` + `Store` + settings.
//! Lock helpers never open a store; the caller controls eval-state lifecycle.
//! Wrap construction + validation in [`with_lock_scope`] so the lazy
//! `«nix-internal»/derivation-internal.nix` load nests under the activity.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use devenv_activity::{Activity, ActivityLevel, instrument_activity};
use devenv_core::config::Input;
use devenv_core::nix_log_bridge::NixLogBridge;
use miette::{Result, WrapErr};
use nix_bindings_expr::eval_state::{EvalState, EvalStateBuilder};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::store::Store;

use crate::anyhow_ext::AnyhowToMiette;

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
