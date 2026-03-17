# MCP Server

`devenv mcp` launches a [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server that exposes devenv functionality to AI assistants.

## Usage

### Stdio mode (default)

```shell-session
$ devenv mcp
```

The server communicates over stdin/stdout. This is the mode used when an AI tool spawns devenv as a subprocess.

### HTTP mode

```shell-session
$ devenv mcp --http
$ devenv mcp --http 9090
```

Starts the MCP server as an HTTP service. The default port is 8080.

## Available tools

The MCP server provides:

- **search_packages** &mdash; search for packages in the nixpkgs input.
- **search_options** &mdash; search for devenv configuration options.

## Integration with Claude Code

See [Claude Code integration](integrations/claude-code.md) for instructions on configuring Claude Code to use the devenv MCP server automatically.
