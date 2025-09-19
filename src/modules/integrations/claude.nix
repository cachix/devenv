{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.claude.code;

  # Build hooks configuration
  buildHooks =
    hookType: hooks:
    if hooks == [ ] then
      null
    else
      map
        (hook: {
          matcher = hook.matcher or "";
          hooks = [
            {
              type = "command";
              command = hook.command;
            }
          ];
        })
        hooks;

  # Check if git-hooks task is defined (indicates git-hooks are enabled)
  anyGitHooksEnabled = (config.tasks."devenv:git-hooks:run".exec or null) == "pre-commit run -a";

  # Auto-format hook using pre-commit if any git-hooks are enabled
  preCommitHook = lib.optional anyGitHooksEnabled {
    matcher = "^(Edit|MultiEdit|Write)$";
    command = ''
      cd "$DEVENV_ROOT" && pre-commit run
    '';
  };

  # Collect all hooks by type
  allHooks = lib.mapAttrsToList
    (
      name: hook:
        lib.mkIf hook.enable {
          type = hook.hookType;
          hook = {
            matcher = hook.matcher;
            command = hook.command;
          };
        }
    )
    cfg.hooks;

  # Group hooks by type
  groupedHooks = lib.mapAttrs (k: v: map (h: h.hook) v) (
    lib.groupBy (h: h.type) (lib.filter (h: h != false) allHooks)
  );

  # Add pre-commit hook if git-hooks are enabled
  postToolUseHooks = (groupedHooks.PostToolUse or [ ]) ++ preCommitHook;

  # Build MCP servers configuration
  mcpServers = lib.mapAttrs (name: server: 
    if server.type == "stdio" then 
      if server.command == null then
        throw "MCP server '${name}' of type 'stdio' requires a command"
      else {
        type = "stdio";
        command = server.command;
      } // lib.optionalAttrs (server.args != []) {
        args = server.args;
      } // lib.optionalAttrs (server.env != {}) {
        env = server.env;
      }
    else if server.type == "http" then
      if server.url == null then
        throw "MCP server '${name}' of type 'http' requires a url"
      else {
        type = "http";
        url = server.url;
      }
    else throw "Invalid MCP server type: ${server.type}"
  ) cfg.mcpServers;

  # Generate the settings content
  settingsContent = lib.filterAttrs (n: v: v != null) {
    hooks = lib.filterAttrs (n: v: v != null) {
      PreToolUse = buildHooks "PreToolUse" (groupedHooks.PreToolUse or [ ]);
      PostToolUse = buildHooks "PostToolUse" postToolUseHooks;
      Notification = buildHooks "Notification" (groupedHooks.Notification or [ ]);
      Stop = buildHooks "Stop" (groupedHooks.Stop or [ ]);
      SubagentStop = buildHooks "SubagentStop" (groupedHooks.SubagentStop or [ ]);
    };
    inherit (cfg)
      apiKeyHelper
      model
      forceLoginMethod
      cleanupPeriodDays
      ;
    env = if cfg.env == { } then null else cfg.env;
    permissions = if cfg.permissions == { } then null else cfg.permissions;
  };

  # Generate the MCP configuration content
  mcpContent = if cfg.mcpServers == {} then null else {
    mcpServers = mcpServers;
  };
in
{
  options.claude.code = {
    enable = lib.mkEnableOption "Claude Code integration with automatic hooks and commands setup";

    hooks = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            enable = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Whether to enable this hook.";
            };
            name = lib.mkOption {
              type = lib.types.str;
              description = "The name of the hook (appears in logs).";
            };
            hookType = lib.mkOption {
              type = lib.types.enum [
                "PreToolUse"
                "PostToolUse"
                "Notification"
                "Stop"
                "SubagentStop"
              ];
              default = "PostToolUse";
              description = ''
                The type of hook:
                - PreToolUse: Runs before tool calls (can block them)
                - PostToolUse: Runs after tool calls complete
                - Notification: Runs when Claude Code sends notifications
                - Stop: Runs when Claude Code finishes responding
                - SubagentStop: Runs when subagent tasks complete
              '';
            };
            matcher = lib.mkOption {
              type = lib.types.str;
              default = "";
              description = "Regex pattern to match against tool names (for PreToolUse/PostToolUse hooks).";
            };
            command = lib.mkOption {
              type = lib.types.str;
              description = "The command to execute.";
            };
          };
        }
      );
      default = { };
      description = ''
        Hooks that run at different points in Claude Code's workflow.
      '';
      example = lib.literalExpression ''
        {
          protect-secrets = {
            enable = true;
            name = "Protect sensitive files";
            hookType = "PreToolUse";
            matcher = "^(Edit|MultiEdit|Write)$";
            command = '''
              json=$(cat);
              file_path = $(echo "$json" | jq - r '.file_path // empty');
              grep -q 'SECRET\\|PASSWORD\\|API_KEY' "$file_path" && echo 'Blocked: sensitive data detected' && exit 1 || exit 0
            ''';
          };
          run-tests = {
            enable = true;
            name = "Run tests after edit";
            hookType = "PostToolUse";
            matcher = "^(Edit|MultiEdit|Write)$";
            command = "cargo test";
          };
          log-completion = {
            enable = true;
            name = "Log when Claude finishes";
            hookType = "Stop";
            command = "echo 'Claude finished responding' >> claude.log";
          };
        }
      '';
    };

    commands = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Custom Claude Code slash commands to create in the project.
        Commands are invoked with `/command-name` in Claude Code.
      '';
      example = lib.literalExpression ''
        {
          test = '''
            Run all tests in the project

            ```bash
            cargo test
            ```
          ''';
          fmt = '''
            Format all code in the project

            ```bash
            cargo fmt
            nixfmt **/*.nix
            ```
          ''';
        }
      '';
    };

    agents = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            description = lib.mkOption {
              type = lib.types.str;
              description = "What the sub-agent does";
            };
            proactive = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Whether Claude should use this sub-agent automatically";
            };
            tools = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "List of allowed tools for this sub-agent";
            };
            prompt = lib.mkOption {
              type = lib.types.lines;
              description = "The system prompt for the sub-agent";
            };
          };
        }
      );
      default = { };
      description = ''
        Custom Claude Code sub-agents to create in the project.
        Sub-agents are specialized AI assistants that handle specific tasks
        with their own context window and can be invoked automatically or explicitly.
        
        For more details, see: https://docs.anthropic.com/en/docs/claude-code/sub-agents
      '';
      example = lib.literalExpression ''
        {
          code-reviewer = {
            description = "Expert code review specialist that checks for quality, security, and best practices";
            proactive = true;
            tools = [ "Read" "Grep" "TodoWrite" ];
            prompt = '''
              You are an expert code reviewer. When reviewing code, check for:
              - Code readability and maintainability
              - Proper error handling
              - Security vulnerabilities
              - Performance issues
              - Adherence to project conventions
              
              Provide constructive feedback with specific suggestions for improvement.
            ''';
          };
          
          test-writer = {
            description = "Specialized in writing comprehensive test suites";
            proactive = false;
            tools = [ "Read" "Write" "Edit" "Bash" ];
            prompt = '''
              You are a test writing specialist. Create comprehensive test suites that:
              - Cover edge cases and error conditions
              - Follow the project's testing conventions
              - Include unit, integration, and property-based tests where appropriate
              - Have clear test names that describe what is being tested
            ''';
          };
        }
      '';
    };

    apiKeyHelper = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        Custom script for generating authentication tokens.
        The script should output the API key to stdout.
      '';
      example = "aws secretsmanager get-secret-value --secret-id claude-api-key | jq -r .SecretString";
    };

    model = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        Override the default Claude model.
      '';
      example = "claude-3-opus-20240229";
    };

    forceLoginMethod = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.enum [
          "browser"
          "api-key"
        ]
      );
      default = null;
      description = ''
        Restrict the login method to either browser or API key authentication.
      '';
    };

    cleanupPeriodDays = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null;
      description = ''
        Retention period for chat transcripts in days.
      '';
      example = 30;
    };

    env = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Custom environment variables for Claude Code sessions.
      '';
      example = {
        PYTHONPATH = "/custom/python/path";
        NODE_ENV = "development";
      };
    };

    permissions = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            allow = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "List of allowed tools or patterns.";
            };
            deny = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "List of denied tools or patterns.";
            };
          };
        }
      );
      default = { };
      description = ''
        Fine-grained permissions for tool usage.
        Can specify allow/deny rules for different tools.
      '';
      example = lib.literalExpression ''
        {
          Edit = {
            deny = [ "*.secret" "*.env" ];
          };
          Bash = {
            deny = [ "rm -rf" ];
          };
        }
      '';
    };

    mcpServers = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            type = lib.mkOption {
              type = lib.types.enum [ "stdio" "http" ];
              description = "Type of MCP server connection.";
            };
            command = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "Command to execute for stdio MCP servers.";
            };
            args = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "Arguments to pass to the command for stdio MCP servers.";
            };
            env = lib.mkOption {
              type = lib.types.attrsOf lib.types.str;
              default = { };
              description = "Environment variables for stdio MCP servers.";
            };
            url = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "URL for HTTP MCP servers.";
            };
          };
        }
      );
      default = { };
      description = ''
        MCP (Model Context Protocol) servers to configure.
        These servers provide additional capabilities and context to Claude Code.
      '';
      example = lib.literalExpression ''
        {
          awslabs-iam-mcp-server = {
            type = "stdio";
            command = lib.getExe pkgs.awslabs-iam-mcp-server;
            args = [ ];
            env = { };
          };
          linear = {
            type = "http";
            url = "https://mcp.linear.app/mcp";
          };
          devenv = {
            type = "stdio";
            command = "devenv";
            args = [ "mcp" ];
            env = {
              DEVENV_ROOT = config.devenv.root;
            };
          };
        }
      '';
    };

    settingsPath = lib.mkOption {
      type = lib.types.str;
      default = "${config.devenv.root}/.claude/settings.json";
      internal = true;
      description = ''
        Path to the Claude Code settings file within the repository.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    files = lib.mkMerge [
      { ".claude/settings.json".json = settingsContent; }
      
      # MCP configuration file
      (lib.mkIf (cfg.mcpServers != {}) {
        ".mcp.json".json = mcpContent;
      })

      # Command files
      (lib.mapAttrs'
        (name: content: {
          name = ".claude/commands/${name}.md";
          value = {
            text = content;
          };
        })
        cfg.commands)

      # Sub-agent files
      (lib.mapAttrs'
        (name: agent: {
          name = ".claude/agents/${name}.md";
          value = {
            text = ''
              ---
              name: ${name}
              description: ${agent.description}
              proactive: ${lib.boolToString agent.proactive}
              ${lib.optionalString (agent.tools != []) "tools:\n${lib.concatMapStringsSep "\n" (tool: "  - ${tool}") agent.tools}"}
              ---

              ${agent.prompt}
            '';
          };
        })
        cfg.agents)
    ];

    # Add a message about the integration
    infoSections."claude" = [
      ''
        Claude Code integration is enabled with automatic hooks and commands setup.
        Settings are configured at: ${cfg.settingsPath}
        ${lib.optionalString anyGitHooksEnabled "- Auto-formatting: enabled via git-hooks (pre-commit)"}
        ${lib.optionalString (cfg.commands != { })
          "- Project commands: ${
            lib.concatStringsSep ", " (map (cmd: "/${cmd}") (lib.attrNames cfg.commands))
          }"
        }
        ${lib.optionalString (cfg.agents != { })
          "- Sub-agents: ${
            lib.concatStringsSep ", " (lib.attrNames cfg.agents)
          }"
        }
        ${lib.optionalString (cfg.mcpServers != { })
          "- MCP servers: ${
            lib.concatStringsSep ", " (lib.attrNames cfg.mcpServers)
          } (configured at ${config.devenv.root}/.mcp.json)"
        }
      ''
    ];
  };
}
