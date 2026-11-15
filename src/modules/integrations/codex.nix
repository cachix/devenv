{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.codex;
  toml = pkgs.formats.toml { };

  hookTypes = [
    "PermissionRequest"
    "PostCompact"
    "PostModelRequest"
    "PostToolUse"
    "PreCompact"
    "PreModelRequest"
    "PreToolUse"
    "SessionEnd"
    "SessionStart"
    "SubagentStart"
    "SubagentStop"
    "UserPromptSubmit"
    "Stop"
    "Interrupt"
  ];

  hookModule =
    { name, ... }:
    {
      options = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Whether to enable the ${name} hook.";
        };

        name = lib.mkOption {
          type = lib.types.str;
          default = name;
          description = "A human-readable name for the hook.";
        };

        hookType = lib.mkOption {
          type = lib.types.enum hookTypes;
          description = "The Codex lifecycle event that runs this hook.";
        };

        matcher = lib.mkOption {
          type = lib.types.str;
          default = "";
          description = "Regex matched against the event-specific value, such as a tool name.";
        };

        command = lib.mkOption {
          type = lib.types.str;
          description = "Command to execute.";
        };

        commandWindows = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Optional Windows command override.";
        };

        timeout = lib.mkOption {
          type = lib.types.nullOr lib.types.ints.unsigned;
          default = null;
          description = "Optional hook timeout in seconds.";
        };

        statusMessage = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Optional status message shown while the hook runs.";
        };

        additionalContextLimit = lib.mkOption {
          type = lib.types.nullOr lib.types.ints.unsigned;
          default = null;
          description = "Maximum approximate token count for model-visible additional context.";
        };

        async = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Whether Codex should run the hook in the background.";
        };
      };
    };

  enabledHooks = lib.filterAttrs (_: hook: hook.enable) cfg.hooks;
  groupedHooks = lib.groupBy (hook: hook.hookType) (lib.attrValues enabledHooks);

  generatedHooks = lib.mapAttrs (
    _: hooks:
    map (hook: {
      inherit (hook) matcher;
      hooks = [
        (lib.filterAttrs (_: value: value != null && value != false) {
          type = "command";
          inherit (hook)
            command
            timeout
            statusMessage
            additionalContextLimit
            async
            ;
          command_windows = hook.commandWindows;
        })
      ];
    }) hooks
  ) groupedHooks;

  mcpServerModule = lib.types.submodule {
    options = {
      type = lib.mkOption {
        type = lib.types.enum [
          "stdio"
          "http"
        ];
        description = "MCP server transport.";
      };

      command = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Command for a stdio MCP server.";
      };

      args = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "Arguments for a stdio MCP server.";
      };

      env = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        description = "Environment variables for a stdio MCP server.";
      };

      url = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "URL for an HTTP MCP server.";
      };

      headers = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        description = "Static HTTP headers.";
      };

      settings = lib.mkOption {
        type = toml.type;
        default = { };
        description = "Additional native Codex MCP server settings.";
      };
    };
  };

  buildMcpServer =
    name: server:
    let
      transport =
        if server.type == "stdio" then
          if server.command == null then
            throw "MCP server '${name}' of type 'stdio' requires a command"
          else
            {
              command = server.command;
            }
            // lib.optionalAttrs (server.args != [ ]) { inherit (server) args; }
            // lib.optionalAttrs (server.env != { }) { inherit (server) env; }
        else if server.url == null then
          throw "MCP server '${name}' of type 'http' requires a url"
        else
          {
            url = server.url;
          }
          // lib.optionalAttrs (server.headers != { }) { http_headers = server.headers; };
    in
    transport // server.settings;

  generatedMcpServers = lib.mapAttrs buildMcpServer cfg.mcpServers;
  mergedMcpServers = generatedMcpServers // (cfg.settings.mcp_servers or { });
  mergedHooks = lib.recursiveUpdate generatedHooks (cfg.settings.hooks or { });

  settings =
    builtins.removeAttrs cfg.settings [
      "hooks"
      "mcp_servers"
    ]
    // lib.optionalAttrs (mergedHooks != { }) { hooks = mergedHooks; }
    // lib.optionalAttrs (mergedMcpServers != { }) { mcp_servers = mergedMcpServers; };

  agentModule =
    { name, ... }:
    {
      options = {
        description = lib.mkOption {
          type = lib.types.str;
          description = "When Codex should use the ${name} agent.";
        };

        prompt = lib.mkOption {
          type = lib.types.either lib.types.lines lib.types.path;
          description = "Developer instructions for the ${name} agent.";
        };

        model = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Optional model override for the agent.";
        };

        effort = lib.mkOption {
          type = lib.types.nullOr (
            lib.types.enum [
              "low"
              "medium"
              "high"
              "xhigh"
              "max"
              "ultra"
            ]
          );
          default = null;
          description = "Optional model reasoning effort for the agent.";
        };

        settings = lib.mkOption {
          type = toml.type;
          default = { };
          description = "Additional native Codex configuration for the agent.";
        };
      };
    };

  skillResourceType = lib.types.coercedTo lib.types.path (source: {
    inherit source;
  }) config.lib.fileSpecType;
  fileCopyModeType = config.lib.fileCopyModeType;

  skillModule =
    { name, ... }:
    {
      options = {
        description = lib.mkOption {
          type = lib.types.str;
          description = "What the skill covers and when Codex should load it.";
        };

        content = lib.mkOption {
          type = lib.types.lines;
          description = "The body of SKILL.md, loaded when the skill triggers.";
        };

        resources = lib.mkOption {
          type = lib.types.attrsOf skillResourceType;
          default = { };
          description = "Files bundled alongside SKILL.md, keyed by relative path.";
          example = lib.literalExpression ''
            {
              "references/api.md" = ./api.md;
              "scripts/check.sh" = { source = ./check.sh; executable = true; };
            }
          '';
        };

        copyMode = lib.mkOption {
          type = fileCopyModeType;
          default = "symlink";
          description = "How to materialise the skill files.";
        };
      };
    };
in
{
  options.codex = {
    enable = lib.mkEnableOption "Codex integration with automatic hooks, agents, and skills setup";

    settings = lib.mkOption {
      type = toml.type;
      default = { };
      description = "Native Codex settings written to `.codex/config.toml`.";
      example = lib.literalExpression ''
        {
          model = "gpt-6.1-sol";
          model_reasoning_effort = "high";
          sandbox_mode = "workspace-write";
        }
      '';
    };

    hooks = lib.mkOption {
      type = lib.types.submodule {
        freeformType = lib.types.attrsOf (lib.types.submodule hookModule);
        options.git-hooks-run = lib.mkOption {
          type = lib.types.submodule hookModule;
          default = {
            enable = config.git-hooks.enable;
            name = "Run git-hooks";
            hookType = "PostToolUse";
            matcher = "^(apply_patch|Edit|Write)$";
            command =
              if config.git-hooks.enable then
                ''cd "$DEVENV_ROOT" && ${config.git-hooks.package.meta.mainProgram} run''
              else
                "true";
            statusMessage = "Running git hooks";
          };
          defaultText = lib.literalExpression ''
            {
              enable = config.git-hooks.enable;
              name = "Run git-hooks";
              hookType = "PostToolUse";
              matcher = "^(apply_patch|Edit|Write)$";
              command = "cd \"$DEVENV_ROOT\" && \''${config.git-hooks.package.meta.mainProgram} run";
              statusMessage = "Running git hooks";
            }
          '';
          description = ''
            Automatically runs git-hooks after Codex edits files.
            Enabled by default when `git-hooks.enable` is true.
          '';
        };
      };
      default = { };
      description = "Hooks that run at different points in Codex's lifecycle.";
      example = lib.literalExpression ''
        {
          run-tests = {
            hookType = "PostToolUse";
            matcher = "^(apply_patch|Edit|Write)$";
            command = "cargo test";
            timeout = 120;
            statusMessage = "Running tests";
          };
        }
      '';
    };

    mcpServers = lib.mkOption {
      type = lib.types.attrsOf mcpServerModule;
      default = {
        "mcp.devenv.sh" = {
          type = "http";
          url = "https://mcp.devenv.sh";
        };
      };
      description = "MCP servers configured in `.codex/config.toml`.";
      example = lib.literalExpression ''
        {
          devenv = {
            type = "stdio";
            command = "devenv";
            args = [ "mcp" ];
          };

          docs = {
            type = "http";
            url = "https://mcp.example.com";
            settings.bearer_token_env_var = "MCP_TOKEN";
          };
        }
      '';
    };

    agents = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule agentModule);
      default = { };
      description = ''
        Reusable Codex agents written to `.codex/agents/<name>.toml`.
        Agents inherit the parent session's permissions, provider, and MCP servers.
      '';
      example = lib.literalExpression ''
        {
          reviewer = {
            description = "Review changes for correctness and regressions.";
            model = "gpt-6.1-sol";
            effort = "high";
            prompt = "Focus on actionable findings.";
            settings.features.shell_tool = false;
          };
        }
      '';
    };

    skills = lib.mkOption {
      type = lib.types.attrsOf (lib.types.either lib.types.path (lib.types.submodule skillModule));
      default = { };
      description = ''
        Project skills written to `.agents/skills/<name>`.

        A value can be a complete skill directory or a structured skill with
        generated frontmatter, content, and bundled resources.
      '';
      example = lib.literalExpression ''
        {
          existing = ./skills/existing;
          test = {
            description = "Run the project's focused test workflow.";
            content = "Use `devenv test` and report failures with their logs.";
            resources."references/testing.md" = ./docs/testing.md;
          };
        }
      '';
    };

    settingsPath = lib.mkOption {
      type = lib.types.str;
      default = "${config.devenv.root}/.codex/config.toml";
      internal = true;
      description = "Path to the Codex settings file within the repository.";
    };
  };

  config = lib.mkMerge [
    {
      assertions = lib.mapAttrsToList (name: skill: {
        assertion =
          builtins.match "[a-z0-9]+(-[a-z0-9]+)*" name != null
          && builtins.stringLength name <= 64
          && (!lib.isPath skill || lib.pathIsDirectory skill);
        message = ''
          codex.skills.${name}: skill names must use lowercase letters, digits, and
          single hyphens (at most 64 characters), and path values must be directories.
        '';
      }) cfg.skills;
    }

    (lib.mkIf cfg.enable {
      files = lib.mkMerge [
        {
          "${cfg.settingsPath}".toml = settings;
        }

        (lib.mapAttrs' (
          name: agent:
          lib.nameValuePair ".codex/agents/${name}.toml" {
            toml =
              agent.settings
              // lib.optionalAttrs (agent.model != null) { inherit (agent) model; }
              // lib.optionalAttrs (agent.effort != null) { model_reasoning_effort = agent.effort; }
              // {
                inherit name;
                inherit (agent) description;
                developer_instructions =
                  if lib.isPath agent.prompt then builtins.readFile agent.prompt else agent.prompt;
              };
          }
        ) cfg.agents)

        (lib.mapAttrs' (
          name: skill:
          if lib.isPath skill then
            lib.nameValuePair ".agents/skills/${name}" { source = skill; }
          else
            lib.nameValuePair ".agents/skills/${name}/SKILL.md" {
              inherit (skill) copyMode;
              text = ''
                ---
                name: ${name}
                description: ${builtins.toJSON skill.description}
                ---

                ${skill.content}
              '';
            }
        ) cfg.skills)

        (lib.listToAttrs (
          lib.concatLists (
            lib.mapAttrsToList (
              name: skill:
              if lib.isPath skill then
                [ ]
              else
                lib.mapAttrsToList (
                  path: resource:
                  lib.nameValuePair ".agents/skills/${name}/${path}" {
                    source = resource.file;
                    inherit (skill) copyMode;
                  }
                ) skill.resources
            ) cfg.skills
          )
        ))
      ];

      infoSections."Codex" = [
        ''
          Codex integration is enabled.
          Settings: ${cfg.settingsPath}
          ${lib.optionalString config.git-hooks.enable "- Auto-formatting: enabled via git-hooks (git-hooks-run)"}
          ${lib.optionalString (
            cfg.agents != { }
          ) "- Agents: ${lib.concatStringsSep ", " (lib.attrNames cfg.agents)}"}
          ${lib.optionalString (
            cfg.skills != { }
          ) "- Skills: ${lib.concatStringsSep ", " (lib.attrNames cfg.skills)}"}
          ${lib.optionalString (
            cfg.mcpServers != { }
          ) "- MCP servers: ${lib.concatStringsSep ", " (lib.attrNames cfg.mcpServers)}"}
        ''
      ];
    })
  ];
}
