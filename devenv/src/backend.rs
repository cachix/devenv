//! Backend factory.
//!
//! Re-exports the per-backend setup primitives so call sites in
//! `devenv::devenv` can compose phase 1 (init nix, open store, lock,
//! compute fingerprint) explicitly before constructing the backend.

pub use devenv_nix_backend::backend::{
    build_lock_eval_state, build_settings, init_nix, open_store,
};
