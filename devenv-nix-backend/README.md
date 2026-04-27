# devenv-nix-backend

devenv's default Nix backend.
Talks to Nix through C++ bindings (the `nix-bindings-*` crates).

A long-lived `EvalState` keeps an in-process evaluation cache across calls within a single devenv run.

## What's in here

- `backend.rs` — `NixCBackend`, the `NixBackend` trait implementation.
- `lib.rs`, `lock.rs` — input locking. Writes `devenv.lock`.
- `cnix_store.rs` — wrapper around the C store API.
- `build_environment.rs` — pure-Rust parser for cached `-env` JSON.
- `cachix_daemon.rs`, `cachix_protocol.rs` — client and wire format for the cachix push daemon.
- `logger.rs` — bridges Nix's activity logger into `tracing`.
- `umask_guard.rs` — scoped restrictive umask around C calls.

## Threads and the GC

Nix uses Boehm GC.
Any thread that touches Nix values has to be registered with it, or parallel marking races and crashes.

- `nix_init()` runs once per process (idempotent).
- `gc_register_current_thread()` registers the caller and stashes the guard in thread-local storage so it lives as long as the thread.
  Tokio worker threads call this from `on_thread_start`.
- `trigger_interrupt()` flips the process-global interrupt flag so an in-progress evaluation aborts on its next check.
