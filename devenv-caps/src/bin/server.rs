/// devenv-cap-server
///
/// A minimal root-privileged process that:
///   - Receives launch requests over an inherited socketpair fd
///   - Forks a child for each request
///   - Sets the requested ambient capabilities on the child
///   - Drops the child to the target UID/GID
///   - Execs the target command in the child
///
/// The server itself holds root only to perform the cap + setuid dance.
/// It never execs anything as root — it always drops privilege first.
///
/// # Security properties
///
/// - No filesystem socket: communication is via an inherited fd from a
///   socketpair, so only the parent process (devenv) can reach the server.
/// - Allowlisted capabilities: the server refuses to grant any capability
///   not in the curated allowlist (see `caps::ALLOWED`).
/// - Tight bounding set: each child's bounding set is restricted to only
///   the capabilities it was granted.
/// - Process tracking: the server only allows signals to PIDs it launched.
///
use rustix::io::{fcntl_getfd, fcntl_setfd, FdFlags};
use rustix::process::{geteuid, kill_process, waitpid, Pid, Signal, WaitOptions};
use std::collections::{HashMap, HashSet};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;

use devenv_caps::caps;
use devenv_caps::drop::{fork_with_caps, ChildSpec};
use devenv_caps::protocol::{self, ProcessExit, Request, Response};

// ---------------------------------------------------------------------------
// Argument parsing (minimal, no external deps)
// ---------------------------------------------------------------------------

struct Args {
    fd: i32,
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut fd = None;
    let mut uid = None;
    let mut gid = None;
    let mut groups = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--fd" => {
                i += 1;
                fd = Some(args[i].parse::<i32>().expect("invalid --fd"));
            }
            "--uid" => {
                i += 1;
                uid = Some(args[i].parse::<u32>().expect("invalid --uid"));
            }
            "--gid" => {
                i += 1;
                gid = Some(args[i].parse::<u32>().expect("invalid --gid"));
            }
            "--groups" => {
                i += 1;
                if !args[i].is_empty() {
                    groups = args[i]
                        .split(',')
                        .map(|s| s.parse::<u32>().expect("invalid --groups"))
                        .collect();
                }
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let uid = uid.unwrap_or_else(|| {
        eprintln!("devenv-cap-server: --uid is required");
        std::process::exit(1);
    });
    let gid = gid.unwrap_or_else(|| {
        eprintln!("devenv-cap-server: --gid is required");
        std::process::exit(1);
    });
    let fd = fd.unwrap_or_else(|| {
        eprintln!("devenv-cap-server: --fd is required");
        std::process::exit(1);
    });

    if uid == 0 || gid == 0 {
        eprintln!("devenv-cap-server: refusing to launch processes as root (uid=0 or gid=0)");
        std::process::exit(1);
    }

    Args {
        fd,
        uid,
        gid,
        groups,
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() {
    // --check: verify we can run as root and exit immediately.
    // Used by the devenv preflight to test sudo access without starting the server.
    if std::env::args().any(|a| a == "--check") {
        if !geteuid().is_root() {
            eprintln!("devenv-cap-server: must be run as root (via sudo)");
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    if !geteuid().is_root() {
        eprintln!("devenv-cap-server: must be run as root (via sudo)");
        std::process::exit(1);
    }

    let args = parse_args();

    // Validate the inherited fd before taking ownership.
    // SAFETY: We borrow the raw fd to check it is valid. No ownership is taken yet.
    let borrowed = unsafe { std::os::unix::io::BorrowedFd::borrow_raw(args.fd) };
    if fcntl_getfd(borrowed).is_err() {
        eprintln!(
            "devenv-cap-server: --fd {} is not a valid file descriptor",
            args.fd
        );
        std::process::exit(1);
    }

    // Take ownership of the validated fd.
    let mut stream = unsafe { UnixStream::from_raw_fd(args.fd) };

    // Set FD_CLOEXEC so forked children cannot reach the cap-server via this
    // fd after exec. The fd was inherited from devenv with CLOEXEC cleared;
    // we only need it in the server process itself.
    let mut flags = fcntl_getfd(&stream).unwrap_or(FdFlags::empty());
    flags.insert(FdFlags::CLOEXEC);
    let _ = fcntl_setfd(&stream, flags);

    // Track PIDs we've launched so we only allow signals to our children.
    let mut known_pids: HashSet<u32> = HashSet::new();
    // Exit info for children that have exited, pending a Poll request.
    let mut exited_pids: HashMap<u32, ProcessExit> = HashMap::new();

    eprintln!(
        "devenv-cap-server: ready (uid={}, gid={}, groups={:?})",
        args.uid, args.gid, args.groups
    );

    loop {
        let request: Request = match protocol::read_message(&mut stream) {
            Ok(req) => req,
            Err(e) => {
                // Parent closed the connection or protocol error — clean up.
                eprintln!("devenv-cap-server: connection closed ({e}), shutting down");
                kill_all(&known_pids);
                break;
            }
        };

        match request {
            Request::Launch {
                id,
                caps: cap_names,
                command,
                args: cmd_args,
                env,
                working_dir,
            } => {
                // Validate capabilities against the allowlist.
                let parsed_caps = match caps::parse_and_validate(&cap_names) {
                    Ok(c) => c,
                    Err(e) => {
                        let msg = format!("capability validation failed for '{id}': {e}");
                        eprintln!("devenv-cap-server: {msg}");
                        let _ =
                            protocol::write_message(&mut stream, &Response::Error { message: msg });
                        continue;
                    }
                };

                // Fork a child with the requested capabilities.
                match fork_with_caps(&ChildSpec {
                    caps: &parsed_caps,
                    uid: args.uid,
                    gid: args.gid,
                    groups: &args.groups,
                    command: &command,
                    args: &cmd_args,
                    env: &env,
                    working_dir: &working_dir,
                }) {
                    Ok(child) => {
                        eprintln!(
                            "devenv-cap-server: launched '{id}' (pid={}) with caps {:?}",
                            child.pid, cap_names
                        );
                        known_pids.insert(child.pid);
                        let _ = protocol::write_message(
                            &mut stream,
                            &Response::Launched { pid: child.pid },
                        );
                    }
                    Err(e) => {
                        let msg = format!("failed to launch '{id}': {e}");
                        eprintln!("devenv-cap-server: {msg}");
                        let _ =
                            protocol::write_message(&mut stream, &Response::Error { message: msg });
                    }
                }
            }

            Request::Signal { pid, signal } => {
                if known_pids.contains(&pid) {
                    // Validate that the pid fits in i32. A u32 > i32::MAX would
                    // wrap to a negative value, causing kill() to target an entire
                    // process group — a privilege escalation in a root process.
                    let raw_pid = match i32::try_from(pid) {
                        Ok(p) if p > 0 => p,
                        _ => {
                            let msg = format!("pid {pid} cannot be safely represented as pid_t");
                            let _ = protocol::write_message(
                                &mut stream,
                                &Response::Error { message: msg },
                            );
                            continue;
                        }
                    };
                    // Validate signal number. Linux signals are 1..=64.
                    // Signal 0 is a no-op probe (kill -0), which is harmless
                    // since we already restrict to known PIDs.
                    if signal < 0 || signal > 64 {
                        let msg = format!("invalid signal number: {signal}");
                        let _ =
                            protocol::write_message(&mut stream, &Response::Error { message: msg });
                        continue;
                    }
                    // Use libc::kill directly — signal is an arbitrary i32 from
                    // the protocol that may not correspond to a rustix Signal variant.
                    let ret = unsafe { libc::kill(raw_pid, signal) };
                    if ret == 0 {
                        let _ = protocol::write_message(&mut stream, &Response::Ok);
                    } else {
                        let msg = format!(
                            "kill({pid}, {signal}) failed: {}",
                            std::io::Error::last_os_error()
                        );
                        let _ =
                            protocol::write_message(&mut stream, &Response::Error { message: msg });
                    }
                } else {
                    let msg = format!("pid {pid} not tracked by this server");
                    let _ = protocol::write_message(&mut stream, &Response::Error { message: msg });
                }
            }

            Request::Poll => {
                let processes: Vec<protocol::ExitedProcess> = exited_pids
                    .drain()
                    .map(|(pid, exit)| protocol::ExitedProcess { pid, exit })
                    .collect();
                let _ = protocol::write_message(&mut stream, &Response::Exited { processes });
            }

            Request::Shutdown => {
                eprintln!("devenv-cap-server: shutdown requested");
                kill_all(&known_pids);
                let _ = protocol::write_message(&mut stream, &Response::Ok);
                break;
            }
        }

        // Reap any finished children (non-blocking).
        reap_children(&mut known_pids, &mut exited_pids);
    }

    std::process::exit(0);
}

/// Send SIGTERM to all tracked children, wait up to 2 s for graceful exit,
/// then SIGKILL any stragglers.
///
/// Uses `waitpid(None, NOHANG)` which reaps any child of this process. Safe
/// here because `fork_with_caps` is the only place this server ever forks.
fn kill_all(pids: &HashSet<u32>) {
    for &pid in pids {
        if let Some(p) = Pid::from_raw(pid as i32) {
            let _ = kill_process(p, Signal::TERM);
        }
    }

    // Poll until all children have exited or the deadline passes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut remaining: HashSet<u32> = pids.clone();

    while !remaining.is_empty() && std::time::Instant::now() < deadline {
        while let Ok(Some((pid, _status))) = waitpid(None, WaitOptions::NOHANG) {
            remaining.remove(&(pid.as_raw_nonzero().get() as u32));
        }
        if !remaining.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    for &pid in &remaining {
        if let Some(p) = Pid::from_raw(pid as i32) {
            let _ = kill_process(p, Signal::KILL);
        }
    }
}

/// Non-blocking waitpid to reap zombie children, saving their exit info.
///
/// Reaps any child of this process — safe because `fork_with_caps` is the
/// only place this server forks. Stop/continue events are ignored.
fn reap_children(known_pids: &mut HashSet<u32>, exited: &mut HashMap<u32, ProcessExit>) {
    while let Ok(Some((pid, status))) = waitpid(None, WaitOptions::NOHANG) {
        let pid = pid.as_raw_nonzero().get() as u32;
        let exit = if let Some(code) = status.exit_status() {
            ProcessExit::Exited(code)
        } else if let Some(sig) = status.terminating_signal() {
            ProcessExit::Signaled(sig)
        } else {
            // Stopped or continued — not a true exit, leave in known_pids.
            continue;
        };
        known_pids.remove(&pid);
        exited.insert(pid, exit);
    }
}
