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

## Agents

Agents are specialized AI assistants that handle specific tasks with their own context window and can be invoked automatically or explicitly. They're perfect for delegating complex or repetitive tasks.

### Configuration

```nix
{
  claude.code.agents = {
    code-reviewer = {
      description = "Expert code review specialist that checks for quality, security, and best practices";
      proactive = true;  # Claude will use this automatically when appropriate
      tools = [ "Read" "Grep" "TodoWrite" ];
      prompt = ''
        You are an expert code reviewer. When reviewing code, check for:
        - Code readability and maintainability
        - Proper error handling
        - Security vulnerabilities
        - Performance issues
        - Adherence to project conventions
        
        Provide constructive feedback with specific suggestions for improvement.
      '';
    };
    
    test-writer = {
      description = "Specialized in writing comprehensive test suites";
      proactive = false;  # Only invoked explicitly
      tools = [ "Read" "Write" "Edit" "Bash" ];
      prompt = ''
        You are a test writing specialist. Create comprehensive test suites that:
        - Cover edge cases and error conditions
        - Follow the project's testing conventions
        - Include unit, integration, and property-based tests where appropriate
        - Have clear test names that describe what is being tested
      '';
    };
    
    docs-updater = {
      description = "Updates project documentation based on code changes";
      proactive = true;
      tools = [ "Read" "Edit" "Grep" ];
      prompt = ''
        You specialize in keeping documentation up-to-date. When code changes:
        - Update API documentation
        - Ensure examples still work
        - Update configuration references
        - Keep README files current
      '';
    };
  };
}
```

### Properties

- **description**: What the sub-agent does (shown in Claude's agent selection)
- **proactive**: Whether Claude should use this sub-agent automatically when relevant
- **tools**: List of tools the sub-agent can use (restricts access for safety)
- **prompt**: The system prompt that defines the sub-agent's behavior

### Available Tools

Common tools that can be assigned to agents:
- `Read`: Read files
- `Write`: Create new files
- `Edit`/`MultiEdit`: Modify existing files
- `Grep`/`Glob`: Search through code
- `Bash`: Execute commands
- `TodoWrite`: Manage task lists
- `WebFetch`/`WebSearch`: Access web resources

### Usage

**Proactive agents** (with `proactive: true`) are automatically invoked by Claude when their expertise is relevant. For example, the code-reviewer sub-agent will automatically review code after significant changes.

**Non-proactive agents** (with `proactive: false`) must be explicitly requested. You can invoke them by asking Claude to use a specific agent or by describing a task that matches their expertise.

### Best Practices

1. **Limit tool access**: Only give agents the tools they need
2. **Clear descriptions**: Help Claude understand when to use each agent
3. **Focused prompts**: Keep agent prompts specific to their task
4. **Use proactive mode carefully**: Only for agents that should run automatically

For more details on agents, see the [official Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code/sub-agents).

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
