use capctl::caps::{Cap, CapSet, CapState};
use capctl::prctl::{set_securebits, Secbits};
use rustix::process::{geteuid, setsid, Gid, Uid};
use rustix::thread::{set_thread_groups, set_thread_res_gid, set_thread_res_uid};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DropError {
    #[error("capability error: {0}")]
    Cap(String),
    #[error("exec error: {0}")]
    Exec(String),
}

/// Result of a `fork()` call, as seen by the parent.
pub struct LaunchedChild {
    pub pid: u32,
}

/// Everything needed to launch a child process with capabilities.
pub struct ChildSpec<'a> {
    pub caps: &'a [Cap],
    pub uid: u32,
    pub gid: u32,
    pub groups: &'a [u32],
    pub command: &'a str,
    pub args: &'a [String],
    pub env: &'a HashMap<String, String>,
    pub working_dir: &'a Path,
}

/// Fork a child process that:
///   1. Tightens the bounding set
///   2. Sets securebits so caps survive the UID change
///   3. Sets permitted/effective/inheritable to only the requested caps
///   4. Drops to the target uid/gid
///   5. Raises ambient caps
///   6. Clears securebits
///   7. Execs the target command
///
/// This function must be called as root.
///
/// # Safety
/// Uses `fork()`, `setuid()`, `setgid()`, and `exec()`. The child process
/// never returns — it either execs or exits.
pub fn fork_with_caps(spec: &ChildSpec<'_>) -> Result<LaunchedChild, DropError> {
    if !geteuid().is_root() {
        return Err(DropError::Cap("must be root to grant capabilities".into()));
    }

    match unsafe { libc::fork() } {
        -1 => Err(DropError::Cap(format!(
            "fork: {}",
            std::io::Error::last_os_error()
        ))),

        0 => {
            // ---- Child process (root) ----
            // This function never returns.
            child_setup_and_exec(spec);
        }

        pid => {
            // ---- Parent process ----
            Ok(LaunchedChild { pid: pid as u32 })
        }
    }
}

/// Child-side logic. Never returns — either execs or exits with an error code.
fn child_setup_and_exec(spec: &ChildSpec<'_>) -> ! {
    match child_setup_inner(spec) {
        Ok(_) => unreachable!("exec should not return"),
        Err(e) => {
            eprintln!("devenv-cap-server: child setup failed: {e}");
            std::process::exit(126);
        }
    }
}

fn child_setup_inner(spec: &ChildSpec<'_>) -> Result<(), DropError> {
    let caps = spec.caps;
    let uid = spec.uid;
    let gid = spec.gid;
    let groups = spec.groups;
    let command = spec.command;
    let args = spec.args;
    let env = spec.env;
    let working_dir = spec.working_dir;
    // 1. Create a new session (detach from cap-server's process group)
    setsid().map_err(|e| DropError::Cap(format!("setsid: {e}")))?;

    // 2. Tighten the bounding set to only the requested capabilities.
    //    This limits what the child (and its children) can ever acquire.
    for cap in Cap::iter() {
        if !caps.contains(&cap) {
            let _ = capctl::bounding::drop(cap);
        }
    }

    // 3. Set securebits so capabilities survive the UID transition.
    //
    //    KEEP_CAPS:       Don't drop caps when changing UID from 0
    //    NO_SETUID_FIXUP: Don't adjust cap sets on UID changes
    //
    //    We lock neither bit — we'll clear them after setuid.
    set_securebits(Secbits::KEEP_CAPS | Secbits::NO_SETUID_FIXUP)
        .map_err(|e| DropError::Cap(format!("set_securebits: {e}")))?;

    // 4. Set cap state to only the requested capabilities.
    let mut desired = CapSet::empty();
    for &cap in caps {
        desired.add(cap);
    }

    let state = CapState {
        permitted: desired,
        effective: desired,
        inheritable: desired,
    };
    state
        .set_current()
        .map_err(|e| DropError::Cap(format!("capset: {e}")))?;

    // 5. Drop to the target user. Order matters: groups first, then gid, then uid.
    let gid_val = Gid::from_raw(gid);
    let uid_val = Uid::from_raw(uid);
    let gid_groups: Vec<Gid> = groups.iter().map(|&g| Gid::from_raw(g)).collect();

    set_thread_groups(&gid_groups).map_err(|e| DropError::Cap(format!("setgroups: {e}")))?;
    set_thread_res_gid(gid_val, gid_val, gid_val)
        .map_err(|e| DropError::Cap(format!("setresgid: {e}")))?;
    set_thread_res_uid(uid_val, uid_val, uid_val)
        .map_err(|e| DropError::Cap(format!("setresuid: {e}")))?;

    // Verify we actually dropped privilege.
    if geteuid().is_root() {
        return Err(DropError::Cap("still root after setresuid".into()));
    }

    // 6. Raise ambient capabilities. These survive execve for non-root processes.
    for &cap in caps {
        capctl::ambient::raise(cap)
            .map_err(|e| DropError::Cap(format!("ambient raise {cap:?}: {e}")))?;
    }

    // 7. Clear securebits — we no longer need them.
    set_securebits(Secbits::empty())
        .map_err(|e| DropError::Cap(format!("clear securebits: {e}")))?;

    // 8. Set working directory.
    std::env::set_current_dir(working_dir)
        .map_err(|e| DropError::Exec(format!("chdir to {}: {e}", working_dir.display())))?;

    // 9. Build the environment.
    //    Clear inherited env and set only what was passed.
    for (key, _) in std::env::vars() {
        std::env::remove_var(&key);
    }
    for (key, value) in env {
        std::env::set_var(key, value);
    }

    // 10. Exec the target process.
    let c_command = CString::new(command.as_bytes())
        .map_err(|_| DropError::Exec("command contains null byte".into()))?;

    let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);
    c_args.push(c_command.clone());
    for arg in args {
        c_args.push(
            CString::new(arg.as_bytes())
                .map_err(|_| DropError::Exec("argument contains null byte".into()))?,
        );
    }

    let c_env: Vec<CString> = env
        .iter()
        .map(|(k, v)| {
            CString::new(format!("{k}={v}"))
                .map_err(|_| DropError::Exec(format!("env var '{k}' contains null byte")))
        })
        .collect::<Result<_, _>>()?;

    let c_args_ptrs: Vec<*const libc::c_char> = c_args
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    let c_env_ptrs: Vec<*const libc::c_char> = c_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    unsafe {
        libc::execvpe(
            c_command.as_ptr(),
            c_args_ptrs.as_ptr(),
            c_env_ptrs.as_ptr(),
        );
    }

    // execvpe only returns on error
    Err(DropError::Exec(format!(
        "execvpe({command}): {}",
        std::io::Error::last_os_error()
    )))
}
