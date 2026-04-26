//! Hook fired by a backend after each store-path realization.
//!
//! The backend invokes `on_realized` inline on its evaluation thread,
//! once per attribute build (and once per shell-derivation realization
//! in `dev_env`), gated on `!cache_hit`. Implementations must be
//! non-blocking — typically a sync `mpsc::UnboundedSender::send`.
//!
//! No async, no `.await`, no locks held across yields: the call site
//! may be on an FFI-owned evaluator thread that cannot yield to the
//! tokio runtime.

use std::path::PathBuf;

pub trait RealizedPathsObserver: Send + Sync {
    /// Called with newly-realized store paths. Must return quickly and
    /// must not block. The slice is borrowed; clone if you need to keep
    /// it.
    fn on_realized(&self, paths: &[PathBuf]);
}
