//! Pre-serialized arguments for the Nix bootstrap entry point.
//!
//! The framework owns the schema (today: [`crate::nix_args::NixArgs<'a>`],
//! mirroring `bootstrapLib.nix`'s expected arg shape). It serializes once
//! via [`BootstrapArgs::from_serializable`] and shares the resulting
//! `Arc<BootstrapArgs>` with the backend.
//!
//! Backends never see [`crate::nix_args::NixArgs`], secretspec, lock
//! fingerprints, or any other framework concept — the payload is opaque
//! Nix code by the time it crosses the seam.

use miette::Result;

/// Pre-serialized arguments for `import bootstrap/default.nix <args>`.
///
/// `serialized` is the source of truth for cache-key derivation;
/// `for_eval` is the same content with a small set of placeholder
/// strings unquoted (e.g. `"builtins.currentSystem"` →
/// `builtins.currentSystem`) so the expression evaluates correctly when
/// spliced into the bootstrap call.
pub struct BootstrapArgs {
    /// Raw serialized Nix attrset expression. Identity-defining; used
    /// as the eval-cache key seed.
    pub serialized: Box<str>,
    /// Eval-ready expression. Same content as `serialized`, with
    /// framework-recognised placeholder strings unquoted.
    pub for_eval: Box<str>,
}

impl BootstrapArgs {
    /// Serialize any [`serde::Serialize`] value via `ser_nix`, then
    /// derive the eval form by unquoting recognised placeholders.
    pub fn from_serializable<T: serde::Serialize>(value: &T) -> Result<Self> {
        let serialized = ser_nix::to_string(value)
            .map_err(|e| miette::miette!("Failed to serialize bootstrap args: {}", e))?;
        let for_eval = unquote_eval_placeholders(&serialized);
        Ok(Self {
            serialized: serialized.into_boxed_str(),
            for_eval: for_eval.into_boxed_str(),
        })
    }
}

/// `ser_nix` writes Rust strings as Nix string literals. A few values
/// are special: they need to round-trip through serialization (so the
/// cache key is stable text), but on the eval side they have to be
/// real Nix expressions, not string literals.
fn unquote_eval_placeholders(s: &str) -> String {
    s.replace("\"builtins.currentSystem\"", "builtins.currentSystem")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TinyArgs<'a> {
        version: &'a str,
        system: &'a str,
    }

    #[test]
    fn from_serializable_round_trips_a_simple_attrset() {
        let args = TinyArgs {
            version: "1.0.0",
            system: "x86_64-linux",
        };
        let b = BootstrapArgs::from_serializable(&args).expect("serialize");
        assert!(b.serialized.contains("\"1.0.0\""));
        assert!(b.serialized.contains("\"x86_64-linux\""));
    }

    #[test]
    fn for_eval_unquotes_builtins_current_system_placeholder() {
        #[derive(Serialize)]
        struct WithPlaceholder<'a> {
            system: &'a str,
        }
        let args = WithPlaceholder {
            system: "builtins.currentSystem",
        };
        let b = BootstrapArgs::from_serializable(&args).expect("serialize");
        assert!(b.serialized.contains("\"builtins.currentSystem\""));
        assert!(b.for_eval.contains("builtins.currentSystem"));
        assert!(!b.for_eval.contains("\"builtins.currentSystem\""));
    }
}
