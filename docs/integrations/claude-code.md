# Claude Code

[Claude Code](https://github.com/anthropics/claude-code) is Anthropic's official CLI for interacting with Claude AI. The devenv integration provides automatic setup of hooks and commands to enhance your development workflow.

## Global Configuration

You can configure Claude Code globally to use devenv by creating a `~/.claude/CLAUDE.md` file:

```markdown
When devenv.nix doesn't exist and a command/tool is missing, create ad-hoc environment:

    $ devenv -O languages.rust.enable:bool true -O packages:pkgs "mypackage mypackage2" shell -- cli args

When the setup is becomes complex create `devenv.nix` and run commands within:

    $ devenv shell -- cli args

See https://devenv.sh/ad-hoc-developer-environments/
```

This tells Claude to use devenv for running commands, ensuring all tools and dependencies are available.

## Features

- **Automatic code formatting**: Runs `pre-commit` hooks on files after Claude edits them
- **Custom hooks**: Define pre/post actions for Claude's tool usage
- **Project commands**: Create custom slash commands for common tasks
- **Seamless integration**: Works with your existing git-hooks configuration

## Basic Setup

Enable the Claude Code integration in your `devenv.nix`:

```nix
{
  claude.code.enable = true;
}
```

## Automatic Formatting

When you have git-hooks enabled, Claude Code will automatically format files after editing them:

```nix
{
  claude.code.enable = true;

  # Enable formatters via git-hooks
  git-hooks.hooks = {
    rustfmt.enable = true;
    nixfmt.enable = true;
    black.enable = true;
    prettier.enable = true;
  };
}
```

This runs `pre-commit run --files <edited-file>` after Claude edits any file, ensuring consistent formatting.

## Custom Hooks

You can define custom hooks that run at different stages of Claude's workflow:

### Hook Types

- **PreToolUse**: Runs before tool execution (can block actions)
- **PostToolUse**: Runs after tool execution
- **Notification**: Triggers on Claude notifications
- **Stop**: Executes when Claude finishes responding
- **SubagentStop**: Runs when subagent tasks complete

### Examples

```nix
{
  claude.code.hooks = {
    # Protect sensitive files (PreToolUse hook)
    protect-secrets = {
      enable = true;
      name = "Protect sensitive files";
      hookType = "PreToolUse";
      matcher = "^(Edit|MultiEdit|Write)$";
      command = ''
        # Read the JSON input from stdin
        json=$(cat)
        file_path=$(echo "$json" | jq -r '.file_path // empty')

        if [[ "$file_path" =~ \.(env|secret)$ ]]; then
          echo "Error: Cannot edit sensitive files"
          exit 1
        fi
      '';
    };

    # Run tests after changes (PostToolUse hook)
    test-on-save = {
      enable = true;
      name = "Run tests after edit";
      hookType = "PostToolUse";
      matcher = "^(Edit|MultiEdit|Write)$";
      command = ''
        # Read the JSON input from stdin
        json=$(cat)
        file_path=$(echo "$json" | jq -r '.file_path // empty')

        if [[ "$file_path" =~ \.rs$ ]]; then
          cargo test
        fi
      '';
    };

    # Type checking (PostToolUse hook)
    typecheck = {
      enable = true;
      name = "Run type checking";
      hookType = "PostToolUse";
      matcher = "^(Edit|MultiEdit|Write)$";
      command = ''
        # Read the JSON input from stdin
        json=$(cat)
        file_path=$(echo "$json" | jq -r '.file_path // empty')

        if [[ "$file_path" =~ \.ts$ ]]; then
          npm run typecheck
        fi
      '';
    };

    # Log notifications (Notification hook)
    log-notifications = {
      enable = true;
      name = "Log Claude notifications";
      hookType = "Notification";
      command = ''echo "Claude notification received" >> claude.log'';
    };

    # Track completion (Stop hook)
    track-completion = {
      enable = true;
      name = "Track when Claude finishes";
      hookType = "Stop";
      command = ''echo "Claude finished at $(date)" >> claude-sessions.log'';
    };

    # Subagent monitoring (SubagentStop hook)
    subagent-complete = {
      enable = true;
      name = "Log subagent completion";
      hookType = "SubagentStop";
      command = ''echo "Subagent task completed" >> subagent.log'';
    };
  };
}
```

## Custom Commands

Create project-specific slash commands that Claude can use:

```nix
{
  claude.code.commands = {
    test = ''
      Run the test suite

      ```bash
      cargo test
      ```
    '';

    build = ''
      Build the project in release mode

      ```bash
      cargo build --release
      ```
    '';

    deploy = ''
      Deploy to production

      This will build and deploy the application.

      ```bash
      ./scripts/deploy.sh production
      ```
    '';

    db-migrate = ''
      Run database migrations

      ```bash
      diesel migration run
      ```
    '';
  };
}
```

These commands will be available in Claude as `/test`, `/build`, `/deploy`, and `/db-migrate`.

## Hook Input Format

Hooks receive a JSON object via stdin containing the tool information. For file-related tools (Edit/Write), the JSON includes:

```json
{
  "tool": "Edit",
  "file_path": "/path/to/file.rs",
  // ... other tool-specific fields
}
```

You can parse this JSON using `jq` or similar tools to access the data.
