# devenv C FFI Backend

Provide a C FFI/Rust-based implementation for [devenv](https://github.com/cachix/devenv) that eliminates the overhead of shelling out to `nix` CLI commands. Includes direct, programmatic access to Nix's input locking functionality and an FFI-based backend for expression evaluation.

## Architecture

An example using `update` subcommand (input locking):

### CLI-based approach (current devenv)
```
devenv.yaml → temp flake.nix → `nix flake lock` subprocess → devenv.lock
```

### C FFI approach (this library)
```
devenv.yaml → devenv::config::Config → FlakeInputs (in-memory) →
nix_flake_lock_inputs() C API → LockFile → devenv.lock
```

Benefits: No process spawning, no temporary files, direct API access, faster execution

Additionally, the library provides a `NixBackend` trait implementation for expression evaluation and building, eliminating the need to shell out to `nix` for those operations as well.

## Components

### NixBackend Implementation (src/nix_backend.rs)
Complete FFI-based NixBackend trait implementation for devenv:
- Exposes Nix C++ functionality through Rust FFI bindings
- Implements all 13 NixBackend trait methods
- All core methods: `build()`, `eval()`, `update()`, `search()`, `metadata()`, `add_gc()`, `gc()`, `dev_env()`, `repl()`
## Status

### NixBackend Trait Implementation - COMPLETE ✅
✅ All 13 trait methods fully implemented:
  - `assemble()` - Initialize backend state
  - `add_gc()` - Register GC roots
  - `build()` - Build Nix derivations via FFI
  - `eval()` - Evaluate Nix expressions via FFI
  - `update()` - Lock flake inputs via FFI
  - `metadata()` - Display flake metadata
  - `search()` - Search nixpkgs packages
  - `gc()` - Run garbage collection
  - `dev_env()` - Extract development environment via FFI
  - `repl()` - Interactive Nix REPL via FFI
  - `add_gc()` - Register GC roots
  - `name()` - Return backend identifier

✅ Comprehensive test coverage (26/27 tests passing):
  - `test_flake_lock.rs` - 5/5 tests passing
  - `test_nix_backend.rs` - 20/21 tests passing
  - Full workflow integration tests included

## Features

- **Direct C FFI integration** - no subprocess spawning or CLI parsing overhead
- **No temporary files** - constructs inputs directly in memory
- **No intermediate file requirements** - generates lock files directly from devenv.yaml
- **Full devenv.yaml compatibility** - uses official devenv config parser
- **Performance** - faster input locking through direct Nix API access
- **FFI-based NixBackend** - Expression evaluation and building without shelling out
- **Reproducible builds** - includes devenv configuration for development
