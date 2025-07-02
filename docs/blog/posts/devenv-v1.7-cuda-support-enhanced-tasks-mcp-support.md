---
date: 2025-07-03
authors:
  - domenkozar
draft: false
---

# devenv 1.7: CUDA Support, Enhanced Tasks, and MCP support

[devenv 1.7](https://github.com/cachix/devenv/releases/tag/v1.7) brings several practical improvements:

- [CUDA support](#platform-specific-configuration) that works across platforms
- [MCP support](#model-context-protocol-mcp-support) that understands devenv configurations
- [Tasks that only run when their inputs change](#tasks-enhancements)
- [Groundwork for Snix](#progress-on-snix-support), Rust-based Nix implementation

## Progress on Snix Support

We've started work on supporting multiple Nix implementations in devenv. The codebase now includes a backend abstraction layer that will allow users to choose between different Nix implementations.

This architectural change paves the way for integrating [Snix](https://github.com/cachix/snix), our development fork. While the Snix backend isn't functional yet, the groundwork is in place for building out this Rust-based alternative to the C++ Nix implementation. See [PR #1950](https://github.com/cachix/devenv/pull/1950) for implementation details.


## Platform-Specific Configuration

### Configuring CUDA Support

Here's how to enable CUDA support only on Linux systems while keeping your environment working smoothly on macOS:

* CUDA-enabled packages are built with GPU support on Linux
* macOS developers can still work on the same project without CUDA
* The correct CUDA capabilities are set for your target GPUs

```yaml
# devenv.yaml
nixpkgs:
  config:
    allowUnfree: true
    x86_64-linux:
      cudaSupport: true
      cudaCapabilities: ["7.5" "8.6" "8.9"]
```

## Tasks Enhancements

Tasks now skip execution when their input files haven't changed, using the new `execIfModified` option:

```nix
{
  tasks = {
    "frontend:build" = {
      exec = "npm run build";
      execIfModified = [ "src/**/*.tsx" "src/**/*.css" "package.json" ];
    };

    "backend:compile" = {
      exec = "cargo build --release";
      execIfModified = [ "src/**/*.rs" "Cargo.toml" "Cargo.lock" ];
    };
  };
}
```

This dramatically speeds up incremental builds by skipping unnecessary work.

### Namespace-Based Task Execution

Run all tasks within a namespace using prefix matching:

```shell-session
# Run all frontend tasks
$ devenv tasks run frontend
```

## Model Context Protocol (MCP) Support

devenv now includes a built-in MCP server that enables AI assistants like Claude to better understand and generate devenv configurations:

```shell-session
# Start the MCP server
$ devenv mcp
```

AI assistants can now:

* Search for packages and their options
* Understand devenv's configuration format
* Generate valid configurations based on your requirements

## Quality of Life Improvements

- **Shell Integration**: Your shell aliases and functions now work correctly
- **Clean Mode**: Fixed shell corruption when using `--clean`
- **Error Messages**: More helpful error messages when commands fail
- **State Handling**: Automatically recovers from corrupted cache files
- **Direnv Integration**: Fewer unnecessary environment reloads

## Upcoming 1.8 Release

### Standardized Language Tooling Configuration

All language modules will support the same configuration pattern ([PR #1974](https://github.com/cachix/devenv/pull/1974)):

```nix
{
  languages.rust.dev.lsp.enable = false;
  languages.rust.dev.debugger.enable = false;
  languages.rust.dev.linter.enable = false;
  languages.rust.dev.formatter.enable = false;
}
```

### Async Core

Operations that can run in parallel will ([PR #1970](https://github.com/cachix/devenv/pull/1970)).

## Getting Started

Join our [Discord community](https://discord.gg/naMgQehY) to share your experiences and help shape devenv's future.

We're particularly interested in feedback on the standardized language tooling configuration coming in 1.8 - let us know if this approach works for your use cases!

Domen
