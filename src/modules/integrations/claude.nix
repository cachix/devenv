{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.claude.code;

  # Tool permissions submodule (reused for both rules and backward compat)
  toolPermissionsSubmodule = lib.types.submodule {
    options = {
      allow = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "List of allowed patterns.";
      };
      ask = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "List of patterns that require user approval.";
      };
      deny = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "List of denied patterns.";
      };
    };
  };

  # Reserved keys that are not tool names (for backward compat detection)
  reservedPermissionKeys = [ "defaultMode" "disableBypassPermissionsMode" "additionalDirectories" "rules" ];

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

  # Auto-format hook using git-hooks if enabled
  preCommitHook = lib.optional (cfg.runPreCommitOnWrites && config.git-hooks.enable) {
    matcher = "^(Edit|MultiEdit|Write)$";
    command = ''
      cd "$DEVENV_ROOT" && ${config.git-hooks.package.meta.mainProgram} run
    '';
  };

  # Collect all hooks by type
  allHooks = lib.mapAttrsToList
    (
      name: hook: {
        type = hook.hookType;
        hook = {
          matcher = hook.matcher;
          command = hook.command;
        };
      }
    )
    (lib.filterAttrs (name: hook: hook.enable) cfg.hooks);

  # Group hooks by type
  groupedHooks = lib.mapAttrs (k: v: map (h: h.hook) v) (
    lib.groupBy (h: h.type) allHooks
  );

  # Add pre-commit hook if git-hooks are enabled
  postToolUseHooks = (groupedHooks.PostToolUse or [ ]) ++ preCommitHook;

  # Build permissions configuration
  # Transforms per-tool permissions to Claude Code's flat format: Tool(pattern)
  buildPermissions =
    let
      perms = cfg.permissions;
      # Get direct tool attrs (backward compat: permissions.Bash instead of permissions.rules.Bash)
      directToolAttrs = lib.filterAttrs (n: v: !builtins.elem n reservedPermissionKeys && builtins.isAttrs v) perms;
      # Merge rules with direct tool attrs (rules take precedence)
      toolPerms = directToolAttrs // perms.rules;
      flattenTier = tier:
        lib.flatten (
          lib.mapAttrsToList
            (tool: toolPerms:
              map (pattern: "${tool}(${pattern})") (toolPerms.${tier} or [ ])
            )
            toolPerms
        );
      allowList = flattenTier "allow";
      askList = flattenTier "ask";
      denyList = flattenTier "deny";
    in
    if toolPerms == { } && perms.defaultMode == null && perms.disableBypassPermissionsMode == null && perms.additionalDirectories == [ ] then
      null
    else
      lib.filterAttrs (n: v: v != null && v != [ ]) {
        defaultMode = perms.defaultMode;
        disableBypassPermissionsMode = perms.disableBypassPermissionsMode;
        additionalDirectories = if perms.additionalDirectories == [ ] then null else perms.additionalDirectories;
        allow = if allowList == [ ] then null else allowList;
        ask = if askList == [ ] then null else askList;
        deny = if denyList == [ ] then null else denyList;
      };

  # Build MCP servers configuration
  mcpServers = lib.mapAttrs
    (name: server:
      if server.type == "stdio" then
        if server.command == null then
          throw "MCP server '${name}' of type 'stdio' requires a command"
        else {
          type = "stdio";
          command = server.command;
        } // lib.optionalAttrs (server.args != [ ]) {
          args = server.args;
        } // lib.optionalAttrs (server.env != { }) {
          env = server.env;
        }
      else if server.type == "http" then
        if server.url == null then
          throw "MCP server '${name}' of type 'http' requires a url"
        else {
          type = "http";
          url = server.url;
        } // lib.optionalAttrs (server.headers != { }) {
          headers = server.headers;
        }
      else throw "Invalid MCP server type: ${server.type}"
    )
    cfg.mcpServers;

  # Generate the settings content
  settingsContent = lib.filterAttrs (n: v: v != null) {
    hooks = lib.filterAttrs (n: v: v != null) {
      PreToolUse = buildHooks "PreToolUse" (groupedHooks.PreToolUse or [ ]);
      PostToolUse = buildHooks "PostToolUse" postToolUseHooks;
      PostToolUseFailure = buildHooks "PostToolUseFailure" (groupedHooks.PostToolUseFailure or [ ]);
      Notification = buildHooks "Notification" (groupedHooks.Notification or [ ]);
      UserPromptSubmit = buildHooks "UserPromptSubmit" (groupedHooks.UserPromptSubmit or [ ]);
      SessionStart = buildHooks "SessionStart" (groupedHooks.SessionStart or [ ]);
      SessionEnd = buildHooks "SessionEnd" (groupedHooks.SessionEnd or [ ]);
      Stop = buildHooks "Stop" (groupedHooks.Stop or [ ]);
      SubagentStart = buildHooks "SubagentStart" (groupedHooks.SubagentStart or [ ]);
      SubagentStop = buildHooks "SubagentStop" (groupedHooks.SubagentStop or [ ]);
      PreCompact = buildHooks "PreCompact" (groupedHooks.PreCompact or [ ]);
      PermissionRequest = buildHooks "PermissionRequest" (groupedHooks.PermissionRequest or [ ]);
    };
    inherit (cfg)
      apiKeyHelper
      model
      forceLoginMethod
      cleanupPeriodDays
      ;
    env = if cfg.env == { } then null else cfg.env;
    permissions = buildPermissions;
  };

  # Generate the MCP configuration content
  mcpContent = if cfg.mcpServers == { } then null else {
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
                "PostToolUseFailure"
                "Notification"
                "UserPromptSubmit"
                "SessionStart"
                "SessionEnd"
                "Stop"
                "SubagentStart"
                "SubagentStop"
                "PreCompact"
                "PermissionRequest"
              ];
              default = "PostToolUse";
              description = ''
                The type of hook:
                - PreToolUse: Runs before tool calls (can block them)
                - PostToolUse: Runs after tool calls complete
                - PostToolUseFailure: Runs after a tool call fails
                - Notification: Runs when Claude Code sends notifications
                - UserPromptSubmit: Runs when user submits a prompt
                - SessionStart: Runs when a Claude Code session starts
                - SessionEnd: Runs when a Claude Code session ends
                - Stop: Runs when Claude Code finishes responding
                - SubagentStart: Runs when a subagent task starts
                - SubagentStop: Runs when subagent tasks complete
                - PreCompact: Runs before message compaction
                - PermissionRequest: Runs when a permission is requested
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
            model = lib.mkOption {
              type = lib.types.nullOr (lib.types.enum [ "opus" "sonnet" "haiku" ]);
              default = null;
              description = "Override the model for this agent.";
            };
            prompt = lib.mkOption {
              type = lib.types.lines;
              description = "The system prompt for the sub-agent";
            };
            permissionMode = lib.mkOption {
              type = lib.types.nullOr (
                lib.types.enum [
                  "default"
                  "acceptEdits"
                  "plan"
                  "bypassPermissions"
                ]
              );
              default = null;
              description = "Permission mode for this specific sub-agent.";
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
            model = "opus";
            tools = [ "Read" "Grep" "TodoWrite" ];
            permissionMode = "plan";
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
      type = lib.types.submodule {
        freeformType = lib.types.attrsOf toolPermissionsSubmodule;
        options = {
          defaultMode = lib.mkOption {
            type = lib.types.nullOr (
              lib.types.enum [
                "default"
                "acceptEdits"
                "plan"
                "bypassPermissions"
              ]
            );
            default = null;
            description = ''
              Global permission mode for Claude Code.
              - default: Prompts on first use of each tool
              - acceptEdits: Auto-accepts file edits
              - plan: Read-only mode
              - bypassPermissions: Skips all permission prompts
            '';
            example = "acceptEdits";
          };
          disableBypassPermissionsMode = lib.mkOption {
            type = lib.types.nullOr lib.types.bool;
            default = null;
            description = ''
              Security option to prevent the dangerous bypassPermissions mode.
            '';
            example = true;
          };
          additionalDirectories = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = ''
              Allow Claude Code to access directories outside the project root.
            '';
            example = [ "/shared/libs" "/common/configs" ];
          };
          rules = lib.mkOption {
            type = lib.types.attrsOf toolPermissionsSubmodule;
            default = { };
            description = ''
              Per-tool permission rules. Preferred location for tool permissions.
            '';
          };
        };
      };
      default = { };
      description = ''
        Fine-grained permissions for tool usage.
        Supports global settings and per-tool allow/ask/deny rules.
        Tool rules can be placed under `rules` or directly (backward compatible).
      '';
      example = lib.literalExpression ''
        {
          defaultMode = "acceptEdits";
          disableBypassPermissionsMode = true;
          additionalDirectories = [ "/shared/libs" ];
          rules = {
            Edit = {
              deny = [ "*.secret" "*.env" ];
            };
            Bash = {
              allow = [ "ls:*" "cat:*" ];
              ask = [ "git:*" "npm:*" ];
              deny = [ "rm -rf:*" "sudo:*" ];
            };
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
            headers = lib.mkOption {
              type = lib.types.attrsOf lib.types.str;
              default = { };
              description = "HTTP headers for HTTP MCP servers (e.g., for authentication).";
            };
          };
        }
      );
      default = {
        "mcp.devenv.sh" = {
          type = "http";
          url = "https://mcp.devenv.sh";
        };
      };
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
          github = {
            type = "http";
            url = "https://api.githubcopilot.com/mcp/";
            headers = {
              Authorization = "Bearer GITHUB_PAT";
            };
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

    runPreCommitOnWrites = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        	Run pre-commit hooks after every write (Edit, MultiEdit, or Write command).
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    files = lib.mkMerge [
      { ".claude/settings.json".json = settingsContent; }

      # MCP configuration file
      (lib.mkIf (cfg.mcpServers != { }) {
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
              ${lib.optionalString (agent.model != null) "model: ${agent.model}"}
              ${lib.optionalString (agent.permissionMode != null) "permissionMode: ${agent.permissionMode}"}
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
        ${lib.optionalString config.git-hooks.enable "- Auto-formatting: enabled via git-hooks (pre-commit)"}
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
