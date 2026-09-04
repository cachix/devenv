---
title: "Claude Code"
---

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
- **Skills**: Package project knowledge Claude loads on demand
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
- **FileChanged**: Runs when a watched file changes on disk

Each hook can also set a `timeout` (in seconds) to bound how long its command is allowed to run before Claude Code cancels it.

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

    # Reload direnv when .envrc changes (FileChanged hook)
    reload-direnv = {
      enable = true;
      name = "Reload direnv on .envrc changes";
      hookType = "FileChanged";
      # For FileChanged, matcher is a glob (or `|`-separated globs) matched
      # against paths relative to the project root, not a tool-name regex.
      matcher = ".envrc";
      command = "direnv reload";
      timeout = 10;
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
      # "Use proactively" tells Claude to delegate to this agent automatically when relevant
      description = "Expert code review specialist that checks for quality, security, and best practices. Use proactively after code changes.";
      tools = [ "Read" "Grep" "TodoWrite" ];
      model = "opus";
      effort = "high";
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
      description = "Updates project documentation based on code changes. Use proactively when code changes affect documentation.";
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

### Primary Agent

By default, Claude Code's main conversation runs as its built-in general-purpose agent. Set `claude.code.agent` to use a different agent as the primary agent instead:

```nix
{
  claude.code.agent = "code-reviewer";
}
```

`claude.code.agent` accepts either:

- The name of one of your project's `claude.code.agents.<name>` entries. That agent becomes the primary agent, and is removed from the sub-agents list.
- The name of a Claude Code built-in agent (e.g. `"general-purpose"`) that isn't defined under `claude.code.agents`. In this case every configured agent is still available as a sub-agent.

This is reflected in `devenv info`, which reports a `Primary agent: <name>` line and, when other agents remain, a `Sub-agents: <names>` line.

### Properties

- **description**: What the sub-agent does and when Claude should delegate to it.
  Include a phrase like "use proactively" to have Claude invoke the agent automatically when relevant.
- **tools**: List of tools the sub-agent can use (restricts access for safety)
- **model**: Override the model for this agent (`opus`, `sonnet`, `haiku`, `fable`, a full model ID, or `inherit`)
- **effort**: Override the reasoning effort for this agent (`low`, `medium`, `high`, `xhigh`, or `max`)
- **prompt**: The system prompt that defines the sub-agent's behavior
- **permissionMode**: Permission mode for this specific sub-agent (`default`, `acceptEdits`, `plan`, `auto`, `dontAsk`, or `bypassPermissions`)

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

Claude delegates to an agent based on its `description` and the task at hand.
Agents whose description says to use them proactively are invoked automatically when their expertise is relevant.
For example, the code-reviewer sub-agent above will review code after significant changes without being asked.

Any agent can also be requested explicitly, by asking Claude to use it or by describing a task that matches its expertise.

### Best Practices

2. **Clear descriptions**: Help Claude understand when to use each agent
3. **Focused prompts**: Keep agent prompts specific to their task
4. **Ask for proactive use carefully**: Only say "use proactively" in the description of agents that should run automatically

For more details on agents, see the [official Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code/sub-agents).

## Skills

Skills are folders of instructions Claude loads on demand. Only a skill's `description` stays in context; the body is read when Claude decides the skill applies.
That makes them the right place for knowledge that is important when it is relevant and noise the rest of the time (e.g., migration workflows, API conventions, deployment runbooks).

Skills are written to `.claude/skills/<name>/SKILL.md`.

```nix
{
  claude.code.skills = {
    database-migrations = {
      description = "How to write and run migrations in this project. Use when adding, editing or rolling back a migration.";
      content = ''
        Migrations live in `migrations/` and are applied with `diesel migration run`.

        - One logical change per migration; never edit an applied migration.
        - Always write the matching `down.sql`.
        - Run `devenv tasks run db:reset` before opening a pull request.
      '';
    };
  };
}
```

### Properties

- **description**: What the skill covers and when to use it. This is the only part Claude sees before loading the skill, so it decides whether the skill ever triggers.
  Keep it one line and name the situations that should pull it in.
- **content**: The body of `SKILL.md`.
- **allowedTools**: Tools Claude can use without asking permission during the turn that invokes the skill; the grant clears when you send your next message.
  This pre-approves tools rather than restricting them; use **disallowedTools** to take tools away.
- **whenToUse**: Extra context for when Claude should invoke the skill, such as trigger phrases or example requests.
  Appended to **description** in the skill listing and counts toward the 1536-character cap; devenv warns when a skill goes over.
- **disallowedTools**: Tools removed from Claude's available pool while the skill is active, cleared when you send your next message.
  Use it for a skill that should never call a given tool, such as an autonomous loop that must not stop to ask a question.
- **disableModelInvocation**: Prevent Claude from automatically loading the skill. Use for workflows you want to trigger manually with `/<name>`.
- **userInvocable**: Whether you can invoke the skill yourself with `/<name>`. Set to `false` when only Claude should invoke it, for background knowledge rather than a command.
- **argumentHint**: Hint shown during autocomplete to indicate the expected arguments, e.g. `[issue-number]`.
- **arguments**: Named positional arguments, substituted into **content** as `$name` in the order given.
- **model**: Model to use while the skill is active, for the rest of the turn. Accepts an alias, a full model ID, or `inherit`.
  With `context = "fork"` it sets the forked subagent's model instead.
- **effort**: Effort level while the skill is active: `low`, `medium`, `high`, `xhigh` or `max`.
- **context**: Set to `"fork"` to run the skill in a forked subagent context.
- **agent**: Which subagent type to use when `context = "fork"` is set, such as one of [`claude.code.agents`](#agents).
- **background**: Only applies with `context = "fork"`. Set to `false` to wait for the forked subagent's result in the turn that invoked the skill.
- **resources**: Extra files placed next to `SKILL.md`, keyed by path relative to the skill directory.
  A bare path is the common case; use `{ source = ./x; executable = true; }`
  when the file needs to be run directly rather than passed to an interpreter.
  Resources take the same content options as [`files`](/reference/options/#files), so they can also be written inline instead of pointing at a path.
- **copyMode**: How the files are materialised, as in [`files.<name>.copyMode`](/reference/options/#filesnamecopymode). Defaults to `symlink`.

Skill names must be lowercase letters, digits and single hyphens, at most 64 characters.
Devenv asserts this, because Claude Code silently ignores skill directories it cannot match to a valid name.
`synced` is rejected too: Claude Code reserves that directory for the skills it downloads from your claude.ai account.

### Bundled resources

Long reference material belongs in its own file rather than in `content`, so Claude pulls it in only when it needs the detail:

```nix
{
  claude.code.skills.api-conventions = {
    description = "REST conventions for this codebase. Use when adding or changing an HTTP endpoint.";
    allowedTools = [ "Read" "Grep" ];
    resources = {
      "references/error-codes.md" = ./docs/error-codes.md;
      "scripts/lint-endpoints.sh" = {
        source = ./scripts/lint-endpoints.sh;
        executable = true;
      };
    };
    content = ''
      Endpoints are versioned under `/v1`.
      Read references/error-codes.md before inventing a new error code.
    '';
  };
}
```

### Editing a skill in place

By default the generated files are symlinks into the Nix store and cannot be edited.
Set `copyMode = "seed"` to have devenv write the skill once and leave it writable, so you can iterate on the prose and move it back into `devenv.nix` when it settles:

```nix
{
  claude.code.skills.api-conventions = {
    copyMode = "seed";
    # ...
  };
}
```

### Skills, commands or agents?

- A **skill** is knowledge Claude loads by itself when the task matches.
- A **command** is something you invoke explicitly with `/name`.
- An **agent** is a separate context window with its own tools and prompt.

Reach for a skill when you would otherwise repeat the same explanation to Claude across sessions.

For more details, see the [official Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code/skills).

## MCP Servers

MCP (Model Context Protocol) servers provide additional capabilities and context to Claude Code. You can configure both stdio and HTTP-based MCP servers:

```nix
{
  claude.code.mcpServers = {
    # Local devenv MCP server
    devenv = {
      type = "stdio";
      command = "devenv";
      args = [ "mcp" ];
      env = {
        DEVENV_ROOT = config.devenv.root;
      };
    };

    # AWS IAM MCP server
    awslabs-iam-mcp-server = {
      type = "stdio";
      command = lib.getExe pkgs.awslabs-iam-mcp-server;
      args = [ ];
      env = { };
    };

    # HTTP-based MCP server
    linear = {
      type = "http";
      url = "https://mcp.linear.app/mcp";
    };

    # HTTP-based MCP server with authentication
    github = {
      type = "http";
      url = "https://api.githubcopilot.com/mcp/";
      headers = {
        Authorization = "Bearer GITHUB_PAT";
      };
    };
  };
}
```

### Server Types

- **stdio**: Executes a command that communicates via stdin/stdout
  - `command`: The executable to run
  - `args`: Command line arguments (optional)
  - `env`: Environment variables (optional)

- **http**: Connects to an HTTP-based MCP server
  - `url`: The server URL
  - `headers`: HTTP headers for authentication or custom configuration (optional)

When MCP servers are configured, devenv generates a `.mcp.json` file that Claude Code uses to connect to these servers.

## Composable Specialized Agents

The [devenv-ai-agents](https://github.com/cachix/devenv-ai-agents) repository provides a composable collection of specialized agents:

- `code-reviewer`
- `architecture-designer`
- `documentation-writer`
- `devops-specialist`
- `fullstack-developer`
- `quality-assurance`

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
