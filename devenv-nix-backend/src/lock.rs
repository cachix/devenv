//! Pre-bootstrap lock-file helpers.
//!
//! Free functions over an explicit `EvalState` + `Store` + settings.
//! Lock helpers never open a store or build an eval state; the caller
//! controls lifecycle. The expected pattern is to build a fresh
//! transient `EvalState` via `NixCBackend::fresh_eval_state` (or
//! analogous setup), pass it here, and drop it when done.

use std::collections::BTreeMap;
use std::path::Path;

use devenv_activity::instrument_activity;
use devenv_core::config::Input;
use miette::Result;
use nix_bindings_expr::eval_state::EvalState;
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::FlakeSettings;
use nix_bindings_store::store::Store;

use crate::anyhow_ext::AnyhowToMiette;

/// Validate (and create or update if needed) `<root>/devenv.lock`,
/// returning the fingerprint of the resulting lock graph.
#[instrument_activity("Validating lock", kind = evaluate)]
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
