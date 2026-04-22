//! Scoped umask guard for Nix C API calls.
//!
//! Nix's CLI sets `umask(0022)` in `initNix()` (libmain/shared.cc), but we drive libstore via the C API and never go through that entry point.
//! On systems with a permissive default umask, parent-side build setup in `DerivationBuilderImpl::startBuild()` creates files with group-write bits, which Nix then rejects as "suspicious ownership or permission".
//!
//! Caveat: `umask(2)` is process-wide. This is not thread-safe.

pub struct UmaskGuard {
    previous: nix::sys::stat::Mode,
}

impl UmaskGuard {
    pub fn restrictive() -> Self {
        let mask = nix::sys::stat::Mode::from_bits_truncate(0o022);
        Self {
            previous: nix::sys::stat::umask(mask),
        }
    }
}

impl Drop for UmaskGuard {
    fn drop(&mut self) {
        nix::sys::stat::umask(self.previous);
    }
}
