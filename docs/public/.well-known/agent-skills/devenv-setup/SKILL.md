---
name: devenv-setup
description: Set up and configure devenv developer environments. Use when the user wants to create a devenv.nix, add languages, services, packages, or configure a reproducible development environment.
---

# devenv Setup

## When to use

Use this skill when:
- Creating a new `devenv.nix` configuration
- Adding programming languages, services, or packages
- Configuring processes, tasks, or git hooks
- Setting up a reproducible developer environment

## Quick start

Initialize a new project:

```shell
devenv init
```

This creates `devenv.nix` and `devenv.yaml` in the current directory.
For installation instructions, see [https://devenv.sh/getting-started/](https://devenv.sh/getting-started/).

## Configuration

### devenv.nix

The main configuration file. Example:

```nix
{ pkgs, ... }:

{
  packages = [ pkgs.git pkgs.curl ];

  languages.rust.enable = true;

  services.postgres.enable = true;

  processes.server.exec = "cargo run";
}
```

### devenv.yaml

Defines inputs and imports:

```yaml
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
```

## Key features

- **Languages**: `languages.<name>.enable = true;` — supports 50+ languages
- **Services**: `services.<name>.enable = true;` — supports 100+ services (postgres, redis, etc.)
- **Packages**: `packages = [ pkgs.<name> ];` — any package from nixpkgs
- **Processes**: `processes.<name>.exec = "command";` — managed background processes
- **Scripts**: `scripts.<name>.exec = "command";` — custom shell scripts
- **Tasks**: `tasks.<name>.exec = "command";` — DAG-based task execution
- **Git hooks**: `git-hooks.hooks.<name>.enable = true;` — pre-commit hooks
- **Containers**: `containers."<name>" = { ... };` — OCI containers

## Ad-hoc environments

Run commands in a temporary environment without creating files:

```shell
devenv -O languages.rust.enable:bool true shell
devenv -O languages.python.enable:bool true -O packages:pkgs "nodejs git" shell -- node --version
```

## Commands

- `devenv shell` — enter the development shell
- `devenv up` — start all processes
- `devenv test` — run tests
- `devenv build` — build outputs
- `devenv search <query>` — search for packages
- `devenv update` — update all inputs
- `devenv update <name>` — update a specific input

## MCP server

devenv provides an MCP server with `search_packages` and `search_options` tools.
Use it for looking up available packages and configuration options.

```shell
devenv mcp          # stdio mode
devenv mcp --http   # HTTP mode on port 8080
```

To wire it up automatically in a project, add to `devenv.nix`:

```nix
{
  # For Claude Code
  claude.code.mcpServers.devenv = {
    type = "stdio";
    command = "devenv";
    args = [ "mcp" ];
  };

  # For OpenCode
  opencode.mcp.devenv = {
    type = "local";
    command = [ "devenv" "mcp" ];
  };
}
```

A hosted instance is also available at `https://mcp.devenv.sh`.

## Reference

- [Documentation](https://devenv.sh)
- [Configuration options](https://devenv.sh/reference/options/)
- [Source code](https://github.com/cachix/devenv)
