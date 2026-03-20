{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.opencode;
  webCfg = cfg.web;
  jsonFormat = pkgs.formats.json { };
  mkFile = name: content: if lib.isPath content then { source = content; } else { text = content; };

  mergedMcp = cfg.mcp // (cfg.settings.mcp or { });
  settingsWithMcp =
    cfg.settings
    // lib.optionalAttrs (mergedMcp != { }) {
      mcp = mergedMcp;
    };
in
{
  options.opencode = {
    enable = lib.mkEnableOption "OpenCode configuration";

    settings = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = { };
      description = "Attributes written to opencode.jsonc.";
      example = lib.literalExpression ''
        {
          editor = "nvim";
          theme = "dark";
          features = {
            autocomplete = true;
            git_integration = true;
          };
        }
      '';
    };

    mcp = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      default = { };
      description = ''
        MCP servers written to `opencode.jsonc` under `mcp`.

        This option mirrors OpenCode's MCP configuration shape directly
        (`local`/`remote`, `command`, `environment`, `url`, `headers`,
        `oauth`, `enabled`, `timeout`, etc.).

        Values from `opencode.settings.mcp` take precedence over this option
        when both define the same MCP server name.
      '';
      example = lib.literalExpression ''
        {
          my-local = {
            type = "local";
            command = [ "devenv" "mcp" ];
            environment = {
              DEVENV_ROOT = "{env:DEVENV_ROOT}";
            };
          };

          context7 = {
            type = "remote";
            url = "https://mcp.example.com";
            headers = {
              Authorization = "Bearer TOKEN";
            };
          };
        }
      '';
    };

    web = {
      enable = lib.mkEnableOption "opencode web service";

      extraArgs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        example = [
          "--hostname"
          "127.0.0.1"
          "--port"
          "4096"
        ];
        description = ''
          Extra arguments to pass to `opencode web`.

          These arguments override web server options in the configuration file.
        '';
      };
    };

    rules = lib.mkOption {
      type = lib.types.either lib.types.lines lib.types.path;
      default = "";
      description = "Global custom instructions (placed in .opencode/AGENTS.md).";
      example = lib.literalExpression ''
        # Custom Development Rules
        - Always write tests
        - Use conventional commits
        - Document public APIs
      '';
    };

    commands = lib.mkOption {
      type = lib.types.either (lib.types.attrsOf (lib.types.either lib.types.lines lib.types.path)) lib.types.path;
      default = { };
      description = ''
        Custom slash commands (placed in .opencode/commands/).

        This option can either be:
        - an attribute set of commands, or
        - a path to a directory containing command files.
      '';
      example = lib.literalExpression ''
        {
          "fix-tests" = '''
            # Fix Tests Command
            Analyze failing tests and suggest fixes.
          ''';
          "review-pr" = ./commands/review-pr.md;
        }
      '';
    };

    agents = lib.mkOption {
      type = lib.types.either (lib.types.attrsOf (lib.types.either lib.types.lines lib.types.path)) lib.types.path;
      default = { };
      description = ''
        Custom agents (placed in .opencode/agents/).

        This option can either be:
        - an attribute set of agents, or
        - a path to a directory containing agent files.
      '';
      example = lib.literalExpression ''
        {
          "code-reviewer" = '''
            # Code Review Agent
            Review code for best practices and potential issues.
          ''';
          "documentation-writer" = ./agents/doc-writer.md;
        }
      '';
    };

    skills = lib.mkOption {
      type = lib.types.either (lib.types.attrsOf (lib.types.either lib.types.lines (lib.types.either lib.types.path lib.types.str))) lib.types.path;
      default = { };
      description = ''
        Custom skills for opencode.

        This option can either be:
        - an attribute set defining skills, or
        - a path to a directory containing skill folders.

        If an attribute set is used, each value can be:
        - inline content (creates `.opencode/skills/<name>/SKILL.md`)
        - a path to a file (creates `.opencode/skills/<name>/SKILL.md`)
        - a path to a directory (creates `.opencode/skills/<name>/`)
      '';
      example = lib.literalExpression ''
        {
          "debug-helper" = '''
            # Debug Helper Skill
            Helps diagnose and fix bugs systematically.
          ''';
          "api-generator" = ./skills/api-generator.md;
          "full-stack-skill" = ./skills/full-stack;
        }
      '';
    };

    themes = lib.mkOption {
      type = lib.types.either (lib.types.attrsOf (lib.types.either (lib.types.attrsOf lib.types.anything) lib.types.path)) lib.types.path;
      default = { };
      description = ''
        Custom themes for opencode.

        This option can either be:
        - an attribute set defining themes, or
        - a path to a directory containing theme files.
      '';
      example = lib.literalExpression ''
        {
          my-theme = {
            colors = {
              background = "#0f1115";
              foreground = "#d6d9e0";
            };
          };
          custom = ./themes/custom.json;
        }
      '';
    };

    tools = lib.mkOption {
      type = lib.types.either (lib.types.attrsOf (lib.types.either lib.types.lines lib.types.path)) lib.types.path;
      default = { };
      description = ''
        Custom tools for opencode.

        This option can either be:
        - an attribute set defining tools, or
        - a path to a directory containing tool files.
      '';
      example = lib.literalExpression ''
        {
          sample = "export default { name: \"sample-tool\"; };";
          lint = ./tools/lint.ts;
        }
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !lib.isPath cfg.commands || lib.pathIsDirectory cfg.commands;
        message = "`opencode.commands` must be a directory when set to a path.";
      }
      {
        assertion = !lib.isPath cfg.agents || lib.pathIsDirectory cfg.agents;
        message = "`opencode.agents` must be a directory when set to a path.";
      }
      {
        assertion = !lib.isPath cfg.skills || lib.pathIsDirectory cfg.skills;
        message = "`opencode.skills` must be a directory when set to a path.";
      }
      {
        assertion = !lib.isPath cfg.tools || lib.pathIsDirectory cfg.tools;
        message = "`opencode.tools` must be a directory when set to a path.";
      }
      {
        assertion = !lib.isPath cfg.themes || lib.pathIsDirectory cfg.themes;
        message = "`opencode.themes` must be a directory when set to a path.";
      }
    ];

    files = lib.mkMerge [
      {
        "opencode.jsonc".json = {
          "$schema" = "https://opencode.ai/config.json";
        }
        // settingsWithMcp;
      }

      (lib.mkIf (cfg.rules != "") {
        ".opencode/AGENTS.md" = mkFile "AGENTS" cfg.rules;
      })

      (lib.mkIf (lib.isPath cfg.commands) {
        ".opencode/commands" = {
          source = cfg.commands;
        };
      })

      (lib.mkIf (lib.isPath cfg.agents) {
        ".opencode/agents" = {
          source = cfg.agents;
        };
      })

      (lib.mkIf (lib.isPath cfg.skills) {
        ".opencode/skills" = {
          source = cfg.skills;
        };
      })

      (lib.mkIf (lib.isPath cfg.tools) {
        ".opencode/tools" = {
          source = cfg.tools;
        };
      })

      (lib.mkIf (lib.isPath cfg.themes) {
        ".opencode/themes" = {
          source = cfg.themes;
        };
      })

      (lib.optionalAttrs (builtins.isAttrs cfg.commands) (
        lib.mapAttrs' (n: v: lib.nameValuePair ".opencode/commands/${n}.md" (mkFile n v)) cfg.commands
      ))

      (lib.optionalAttrs (builtins.isAttrs cfg.agents) (
        lib.mapAttrs' (n: v: lib.nameValuePair ".opencode/agents/${n}.md" (mkFile n v)) cfg.agents
      ))

      (lib.optionalAttrs (builtins.isAttrs cfg.tools) (
        lib.mapAttrs' (n: v: lib.nameValuePair ".opencode/tools/${n}.ts" (mkFile n v)) cfg.tools
      ))

      (lib.listToAttrs (
        lib.mapAttrsToList (
          name: content:
          if
            (lib.isPath content && lib.pathIsDirectory content)
            || (builtins.isString content && lib.hasInfix "/nix/store/" content)
          then
            {
              name = ".opencode/skills/${name}";
              value = {
                source = content;
              };
            }
          else
            {
              name = ".opencode/skills/${name}/SKILL.md";
              value = mkFile name content;
            }
        ) (if builtins.isAttrs cfg.skills then cfg.skills else { })
      ))

      (lib.optionalAttrs (builtins.isAttrs cfg.themes) (
        lib.mapAttrs' (
          name: content:
          lib.nameValuePair ".opencode/themes/${name}.json" (
            if lib.isPath content then
              {
                source = content;
              }
            else
              {
                source = jsonFormat.generate "opencode-theme-${name}.json" (
                  {
                    "$schema" = "https://opencode.ai/theme.json";
                  }
                  // content
                );
              }
          )
        ) cfg.themes
      ))
    ];

    processes = lib.mkIf webCfg.enable {
      opencode-web = {
        exec = ''
          if ! command -v opencode >/dev/null 2>&1; then
            echo "opencode not found in PATH; install it or disable opencode.web.enable"
            exit 1
          fi
          exec opencode web ${lib.escapeShellArgs webCfg.extraArgs}
        '';
      };
    };

    infoSections."OpenCode" = [
      "Settings: opencode.jsonc"
    ]
    ++ lib.optional (cfg.commands != { }) (
      if builtins.isAttrs cfg.commands then
        "Commands: ${lib.concatStringsSep ", " (lib.attrNames cfg.commands)}"
      else
        "Commands: directory source"
    )
    ++ lib.optional (cfg.agents != { }) (
      if builtins.isAttrs cfg.agents then
        "Agents: ${lib.concatStringsSep ", " (lib.attrNames cfg.agents)}"
      else
        "Agents: directory source"
    )
    ++ lib.optional (cfg.mcp != { }) "MCP servers: ${lib.concatStringsSep ", " (lib.attrNames cfg.mcp)}"
    ++ lib.optional (cfg.tools != { }) (
      if builtins.isAttrs cfg.tools then
        "Tools: ${lib.concatStringsSep ", " (lib.attrNames cfg.tools)}"
      else
        "Tools: directory source"
    )
    ++ lib.optional (cfg.themes != { }) (
      if builtins.isAttrs cfg.themes then
        "Themes: ${lib.concatStringsSep ", " (lib.attrNames cfg.themes)}"
      else
        "Themes: directory source"
    )
    ++ lib.optional (cfg.skills != { }) (
      if builtins.isAttrs cfg.skills then
        "Skills: ${lib.concatStringsSep ", " (lib.attrNames cfg.skills)}"
      else
        "Skills: directory source"
    )
    ++ lib.optional webCfg.enable "Web process: opencode-web";
  };
}
