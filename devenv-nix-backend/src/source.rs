//! Fetching an out-of-tree `--from` source before the configuration is loaded.
//!
//! `--from <flake-ref>` names a project that lives outside the working
//! directory. Its `devenv.yaml` has to be merged by `Config::load`, which reads
//! from disk, so the reference is fetched into the store first and the
//! resulting directory is handed to the loader like a `path:` source.

use std::path::PathBuf;

use miette::{Result, WrapErr, bail};
use nix_bindings_expr::eval_state::EvalStateBuilder;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::store::Store;
use nix_bindings_util::settings;
use tracing::debug;

use crate::anyhow_ext::AnyhowToMiette;
use crate::error::format_eval_error;

/// Fetch the source behind a `--from` flake reference and return the directory
/// that holds its `devenv.nix`.
///
/// This runs before the project's Nix settings are resolved (they may come from
/// the fetched `devenv.yaml`), so it evaluates against the default store with
/// only the experimental features devenv always enables.
///
/// Unlike a `path:` source, the reference is resolved on every run and is not
/// recorded in `devenv.lock`; pass an explicit revision
/// (`github:owner/repo/<rev>`) to pin it.
pub fn fetch(reference: &str) -> Result<PathBuf> {
    // `dir=` selects a subdirectory of the fetched tree. It is a flake-reference
    // attribute that `builtins.fetchTree` silently ignores, so apply it here.
    let (url, subdir) = split_dir_attribute(reference);

    crate::nix_init();
    crate::gc_register_current_thread()
        .to_miette()
        .wrap_err("Failed to register thread with Nix garbage collector")?;
    settings::set("extra-experimental-features", "flakes nix-command")
        .to_miette()
        .wrap_err("Failed to enable experimental features")?;

    let store = Store::open(None, [])
        .to_miette()
        .wrap_err("Failed to open Nix store")?;
    let flake_settings = FlakeSettings::new()
        .to_miette()
        .wrap_err("Failed to create flake settings")?;
    let mut eval_state = EvalStateBuilder::new(store)
        .to_miette()
        .wrap_err("Failed to create eval state builder")?
        .flakes(&flake_settings)
        .to_miette()
        .wrap_err("Failed to configure flakes")?
        .build()
        .to_miette()
        .wrap_err("Failed to build eval state")?;

    // The context is discarded because the fetched source is already realised
    // in the store; only its path is needed.
    let expr = format!(
        "builtins.unsafeDiscardStringContext (builtins.fetchTree {}).outPath",
        nix_string_literal(&url)
    );
    let context = format!("Failed to fetch --from source '{reference}'");
    let value = eval_state
        .eval_from_string(&expr, "«devenv --from»")
        .and_then(|value| eval_state.require_string(&value))
        .map_err(|err| miette::Report::from(format_eval_error(&format!("{err:#}"), &context)))?;

    let mut path = PathBuf::from(value);
    if let Some(subdir) = subdir {
        path.push(subdir);
    }
    debug!(reference, path = %path.display(), "fetched --from source");

    if !path.join("devenv.nix").exists() {
        bail!(
            "--from source '{reference}' has no devenv.nix (looked in {})",
            path.display()
        );
    }

    Ok(path)
}

/// Split a `dir=` query attribute off a flake reference, returning the
/// reference without it and the subdirectory it selected.
fn split_dir_attribute(reference: &str) -> (String, Option<String>) {
    let Some((base, rest)) = reference.split_once('?') else {
        return (reference.to_string(), None);
    };
    // A fragment (`#attr`) always follows the query, so keep it attached to
    // whatever query parameters survive.
    let (query, fragment) = match rest.split_once('#') {
        Some((query, fragment)) => (query, Some(fragment)),
        None => (rest, None),
    };

    let mut dir = None;
    let mut kept = Vec::new();
    for param in query.split('&').filter(|param| !param.is_empty()) {
        match param.split_once('=') {
            Some(("dir", value)) => dir = Some(value.to_string()),
            _ => kept.push(param),
        }
    }

    let mut url = base.to_string();
    if !kept.is_empty() {
        url.push('?');
        url.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }

    // A leading or trailing slash would produce `//` or a trailing separator
    // when joined onto the fetched store path.
    let dir = dir.map(|dir| dir.trim_matches('/').to_string());
    (url, dir.filter(|dir| !dir.is_empty()))
}

/// Quote a string for interpolation into a Nix expression.
fn nix_string_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for c in value.chars() {
        match c {
            '"' | '\\' | '$' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_dir_attribute_without_query() {
        assert_eq!(
            split_dir_attribute("github:cachix/devenv"),
            ("github:cachix/devenv".to_string(), None)
        );
    }

    #[test]
    fn split_dir_attribute_extracts_dir() {
        assert_eq!(
            split_dir_attribute("github:cachix/devenv?dir=examples/simple"),
            (
                "github:cachix/devenv".to_string(),
                Some("examples/simple".to_string())
            )
        );
    }

    #[test]
    fn split_dir_attribute_keeps_other_params() {
        assert_eq!(
            split_dir_attribute("github:cachix/devenv?ref=main&dir=examples/simple&narHash=sha256"),
            (
                "github:cachix/devenv?ref=main&narHash=sha256".to_string(),
                Some("examples/simple".to_string())
            )
        );
    }

    #[test]
    fn split_dir_attribute_keeps_fragment() {
        assert_eq!(
            split_dir_attribute("github:cachix/devenv?dir=examples#simple"),
            (
                "github:cachix/devenv#simple".to_string(),
                Some("examples".to_string())
            )
        );
    }

    #[test]
    fn split_dir_attribute_trims_slashes() {
        assert_eq!(
            split_dir_attribute("github:cachix/devenv?dir=/examples/simple/"),
            (
                "github:cachix/devenv".to_string(),
                Some("examples/simple".to_string())
            )
        );
    }

    #[test]
    fn nix_string_literal_escapes() {
        assert_eq!(nix_string_literal("a\"b\\c$d"), r#""a\"b\\c\$d""#);
    }
}
