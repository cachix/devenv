//! Process file-limit initialization for the embedded Nix backend.
//!
//! The Nix CLI raises its open-file soft limit in `initNix()`. devenv initializes
//! Nix through its C API instead, so it must apply the same process setup itself.

/// Raise the open-file soft limit to the highest safe value.
///
/// This is best effort, matching Nix's own initialization behavior.
pub(crate) fn bump_open_file_limit() {
    #[cfg(unix)]
    if let Err(error) = bump_open_file_limit_unix() {
        tracing::debug!(%error, "Failed to raise the open-file limit");
    }
}

#[cfg(unix)]
fn bump_open_file_limit_unix() -> nix::Result<()> {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};

    let (soft_limit, hard_limit) = getrlimit(Resource::RLIMIT_NOFILE)?;
    let platform_limit = platform_open_file_limit();

    if let Some(target) = desired_soft_limit(soft_limit, hard_limit, platform_limit) {
        setrlimit(Resource::RLIMIT_NOFILE, target, hard_limit)?;
    }

    Ok(())
}

#[cfg(unix)]
fn desired_soft_limit(
    soft_limit: nix::sys::resource::rlim_t,
    hard_limit: nix::sys::resource::rlim_t,
    platform_limit: Option<nix::sys::resource::rlim_t>,
) -> Option<nix::sys::resource::rlim_t> {
    let target = platform_limit.map_or(hard_limit, |limit| limit.min(hard_limit));
    (soft_limit < target).then_some(target)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_open_file_limit() -> Option<nix::sys::resource::rlim_t> {
    None
}

#[cfg(target_os = "macos")]
fn platform_open_file_limit() -> Option<nix::sys::resource::rlim_t> {
    use nix::libc;
    use nix::sys::resource::rlim_t;

    let mut max_files: libc::c_int = 0;
    let mut value_size = std::mem::size_of_val(&max_files);

    // SAFETY: Both output pointers refer to initialized, correctly sized local
    // variables. The final two arguments make this a read-only sysctl query.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.maxfilesperproc".as_ptr(),
            std::ptr::from_mut(&mut max_files).cast(),
            std::ptr::from_mut(&mut value_size),
            std::ptr::null_mut(),
            0,
        )
    };

    if result == 0 && value_size == std::mem::size_of_val(&max_files) && max_files > 0 {
        rlim_t::try_from(max_files).ok()
    } else {
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::desired_soft_limit;
    use nix::sys::resource::RLIM_INFINITY;

    #[test]
    fn raises_to_the_hard_limit_without_a_platform_cap() {
        assert_eq!(desired_soft_limit(256, 4096, None), Some(4096));
    }

    #[test]
    fn uses_the_platform_cap_instead_of_an_infinite_hard_limit() {
        assert_eq!(
            desired_soft_limit(256, RLIM_INFINITY, Some(10_240)),
            Some(10_240)
        );
    }

    #[test]
    fn never_exceeds_the_hard_limit() {
        assert_eq!(desired_soft_limit(256, 4096, Some(10_240)), Some(4096));
    }

    #[test]
    fn preserves_an_already_higher_soft_limit() {
        assert_eq!(
            desired_soft_limit(20_000, RLIM_INFINITY, Some(10_240)),
            None
        );
    }
}
