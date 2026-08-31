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
        description = ''
          List of allowed patterns. An empty string emits a bare tool entry
          (e.g. `WebSearch` rather than `WebSearch(pattern)`), matching the
          tool regardless of input. Use this for tools without a matcher
          format such as `WebSearch` or `AskUserQuestion`.
        '';
      };
      ask = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = ''
          List of patterns that require user approval. An empty string emits
          a bare tool entry (e.g. `WebSearch` rather than `WebSearch(pattern)`).
        '';
      };
      deny = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = ''
          List of denied patterns. An empty string emits a bare tool entry
          (e.g. `AskUserQuestion` rather than `AskUserQuestion(pattern)`),
          which is required to deny tools without a matcher format outright.
        '';
      };
    };
  };

  # Claude Code's default cap on the combined `description` + `when_to_use` text
  # it keeps in the always-on skill listing, per skill.
  skillListingMaxDescChars = 1536;

  # A file bundled next to a skill's SKILL.md. A bare path is the common case, so
  # it coerces; the attribute set form takes the same contents and `executable`
  # options as `files.<name>`, reused through `config.lib.fileSpecType`.
  skillResourceType = lib.types.coercedTo lib.types.path (source: { inherit source; })
    config.lib.fileSpecType;

  # Bound out here because the skills submodule shadows `config` with its own.
  fileCopyModeType = config.lib.fileCopyModeType;

  # Reserved keys that are not tool names (for backward compat detection)
  reservedPermissionKeys = [ "defaultMode" "disableBypassPermissionsMode" "additionalDirectories" "rules" ];

  # Hook submodule type (reused for both freeform hooks and named integrations)
  hookSubmodule = lib.types.submodule {
    options = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether to enable this hook.";
      };
      name = lib.mkOption {
        type = lib.types.str;
        default = "";
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
          "WorktreeCreate"
          "WorktreeRemove"
          "TeammateIdle"
          "TaskCompleted"
          "ConfigChange"
          "FileChanged"
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
          - WorktreeCreate: Runs when a new worktree is created
          - WorktreeRemove: Runs when a worktree is removed
          - TeammateIdle: Runs when a teammate agent becomes idle
          - TaskCompleted: Runs when a task is completed
          - ConfigChange: Runs when configuration changes
          - FileChanged: Runs when a watched file changes on disk (see `matcher`)
        '';
      };
      matcher = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = ''
          For most hook types, a regex pattern to match against tool names
          (for PreToolUse/PostToolUse hooks).

          For `FileChanged`, this is instead a glob pattern (or `|`-separated
          list of glob patterns) matched against file paths relative to the
          project root, e.g. `.env` or `*.md` or `.env|.env.local`. Claude
          Code expands the glob(s) into the set of files it watches, and
          fires the hook whenever one of them changes.
        '';
      };
      command = lib.mkOption {
        type = lib.types.str;
        description = "The command to execute.";
      };
      timeout = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        description = ''
          Seconds to wait before canceling the hook command.
          Defaults to Claude Code's built-in default when unset (600 seconds
          for a command hook, lower for some event types).
        '';
        example = 30;
      };
    };
  };

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
            ({
              type = "command";
              command = hook.command;
            } // lib.optionalAttrs (hook.timeout != null) {
              timeout = hook.timeout;
            })
          ];
        })
        hooks;

  # Collect all hooks by type
  allHooks = lib.mapAttrsToList
    (
      name: hook: {
        type = hook.hookType;
        hook = {
          matcher = hook.matcher;
          command = hook.command;
          timeout = hook.timeout;
        };
      }
    )
    (lib.filterAttrs (name: hook: hook.enable) cfg.hooks);

  # Group hooks by type
  groupedHooks = lib.mapAttrs (k: v: map (h: h.hook) v) (
    lib.groupBy (h: h.type) allHooks
  );

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
              map (pattern: if pattern == "" then tool else "${tool}(${pattern})") (toolPerms.${tier} or [ ])
            )
            toolPerms
        );
      allowList = flattenTier "allow";
      askList = flattenTier "ask";
      denyList = flattenTier "deny";
      disableBypassPermissionsMode = if perms.disableBypassPermissionsMode == true then "disable" else null;
    in
    if toolPerms == { } && perms.defaultMode == null && disableBypassPermissionsMode == null && perms.additionalDirectories == [ ] then
      null
    else
      lib.filterAttrs (n: v: v != null && v != [ ]) {
        defaultMode = perms.defaultMode;
        inherit disableBypassPermissionsMode;
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
      PostToolUse = buildHooks "PostToolUse" (groupedHooks.PostToolUse or [ ]);
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
      WorktreeCreate = buildHooks "WorktreeCreate" (groupedHooks.WorktreeCreate or [ ]);
      WorktreeRemove = buildHooks "WorktreeRemove" (groupedHooks.WorktreeRemove or [ ]);
      TeammateIdle = buildHooks "TeammateIdle" (groupedHooks.TeammateIdle or [ ]);
      TaskCompleted = buildHooks "TaskCompleted" (groupedHooks.TaskCompleted or [ ]);
      ConfigChange = buildHooks "ConfigChange" (groupedHooks.ConfigChange or [ ]);
      FileChanged = buildHooks "FileChanged" (groupedHooks.FileChanged or [ ]);
    };
    inherit (cfg)
      agent
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

    agent = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "The agent to use as Claude Code's primary agent.";
    };

    hooks = lib.mkOption {
      type = lib.types.submodule {
        freeformType = lib.types.attrsOf hookSubmodule;
        options.git-hooks-run = lib.mkOption {
          type = hookSubmodule;
          default = {
            enable = config.git-hooks.enable;
            name = "Run git-hooks";
            hookType = "PostToolUse";
            matcher = "^(Edit|MultiEdit|Write)$";
            command =
              if config.git-hooks.enable then
                ''cd "$DEVENV_ROOT" && ${config.git-hooks.package.meta.mainProgram} run''
              else
                "true";
          };
          defaultText = lib.literalExpression ''
            {
              enable = config.git-hooks.enable;
              name = "Run git-hooks";
              hookType = "PostToolUse";
              matcher = "^(Edit|MultiEdit|Write)$";
              command = "cd \"$DEVENV_ROOT\" && \''${config.git-hooks.package.meta.mainProgram} run";
            }
          '';
          description = ''
            Automatically runs git-hooks after Claude edits files.
            Enabled by default when `git-hooks.enable` is true.
          '';
        };
      };
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
            timeout = 120;
          };
          log-completion = {
            enable = true;
            name = "Log when Claude finishes";
            hookType = "Stop";
            command = "echo 'Claude finished responding' >> claude.log";
          };
          reload-direnv = {
            enable = true;
            name = "Reload direnv on .envrc changes";
            hookType = "FileChanged";
            matcher = ".envrc";
            command = "direnv reload";
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
          imports = [
            (lib.mkRemovedOptionModule [ "proactive" ] ''
              Claude Code has no `proactive` frontmatter field, so `claude.code.agents.<name>.proactive` never had an effect.
              To encourage automatic delegation, include a phrase like "use proactively" in the agent's `description`.
            '')
          ];

          options = {
            description = lib.mkOption {
              type = lib.types.str;
              description = "What the sub-agent does";
            };
            tools = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "List of allowed tools for this sub-agent";
            };
            model = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = ''
                Override the model for this agent.
                Accepts an alias (`sonnet`, `opus`, `haiku`, `fable`), a full model ID (e.g. `claude-opus-5`), or `inherit`.
              '';
              example = "opus";
            };
            effort = lib.mkOption {
              type = lib.types.nullOr (lib.types.enum [ "low" "medium" "high" "xhigh" "max" ]);
              default = null;
              description = "Override the effort level for this agent.";
            };
            prompt = lib.mkOption {
              type = lib.types.lines;
              description = "The system prompt for the sub-agent";
            };
            permissionMode = lib.mkOption {
              type = lib.types.nullOr (
                lib.types.enum [
                  "default"
                  "manual"
                  "acceptEdits"
                  "plan"
                  "auto"
                  "dontAsk"
                  "bypassPermissions"
                ]
              );
              default = null;
              description = ''
                Permission mode for this specific sub-agent.
                `manual` is an alias for `default`.
              '';
            };
            assertions = lib.mkOption {
              type = lib.types.listOf lib.types.unspecified;
              default = [ ];
              internal = true;
              visible = false;
              description = "Assertions raised by this agent's submodule, collected into the top-level assertions.";
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
            description = "Expert code review specialist that checks for quality, security, and best practices. Use proactively after code changes.";
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

    skills = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule ({ name, config, ... }: {
          options = {
            description = lib.mkOption {
              type = lib.types.str;
              description = ''
                What the skill covers and when Claude should load it.
                Descriptions are the only part of a skill kept in context at all
                times, so this is what decides whether the skill ever triggers.
              '';
            };
            whenToUse = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = ''
                Additional context for when Claude should invoke the skill, such as trigger
                phrases or example requests. Appended to `description` in the skill listing
                and counts toward the ${toString skillListingMaxDescChars}-character cap.
              '';
              example = "Use when the user mentions migrations, schema changes or rollbacks.";
            };
            content = lib.mkOption {
              type = lib.types.lines;
              description = "The body of SKILL.md, loaded when the skill triggers.";
            };
            allowedTools = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = ''
                Tools Claude can use without asking permission during the turn that invokes
                this skill. The grant clears when you send your next message.

                This pre-approves tools rather than restricting them; use `disallowedTools`
                to take tools away.
              '';
              example = [ "Read" "Grep" "Bash(git status:*)" ];
            };
            disallowedTools = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = ''
                Tools removed from Claude's available pool while this skill is active; the
                restriction clears when you send your next message. Use it for a skill that
                should never call a given tool, such as an autonomous loop that must not stop
                to ask a question.
              '';
              example = [ "AskUserQuestion" ];
            };
            disableModelInvocation = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = ''
                Prevent Claude from automatically loading this skill. Use for workflows you
                want to trigger manually with `/${name}`.
              '';
            };
            userInvocable = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = ''
                Whether you can invoke the skill yourself with `/${name}`. Set to false when
                only Claude should invoke it, for background knowledge rather than a command.
              '';
            };
            argumentHint = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "Hint shown during autocomplete to indicate the expected arguments.";
              example = "[issue-number]";
            };
            arguments = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = ''
                Named positional arguments, substituted into `content` as `$name`. Names map
                to argument positions in order.
              '';
              example = [ "issue" "branch" ];
            };
            model = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = ''
                Model to use while this skill is active, for the rest of the current turn.
                Accepts an alias (`sonnet`, `opus`, `haiku`, `fable`), a full model ID
                (e.g. `claude-opus-5`), or `inherit`. With `context = "fork"` it sets the
                forked subagent's model instead.
              '';
              example = "opus";
            };
            effort = lib.mkOption {
              type = lib.types.nullOr (lib.types.enum [ "low" "medium" "high" "xhigh" "max" ]);
              default = null;
              description = ''
                Override the effort level while this skill is active.
                Which levels are available depends on the model.
              '';
            };
            context = lib.mkOption {
              type = lib.types.nullOr (lib.types.enum [ "fork" ]);
              default = null;
              description = ''
                Set to `fork` to run the skill in a forked subagent context.
              '';
            };
            agent = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = ''
                Which subagent type to use when `context = "fork"` is set, such as one of
                `claude.code.agents`.
              '';
              example = "code-reviewer";
            };
            background = lib.mkOption {
              type = lib.types.nullOr lib.types.bool;
              default = null;
              description = ''
                Only applies with `context = "fork"`. Set to false to wait for the forked
                subagent's result in the turn that invoked the skill, instead of letting it
                run in the background.
              '';
            };
            resources = lib.mkOption {
              type = lib.types.attrsOf skillResourceType;
              default = { };
              example = lib.literalExpression ''
                {
                  "references/api.md" = ./api.md;
                  "scripts/check.sh" = { source = ./check.sh; executable = true; };
                }
              '';
              description = ''
                Files placed alongside SKILL.md, keyed by path relative to the
                skill directory. Claude reads these on demand, so bulky reference
                material belongs here rather than in `content`.
              '';
            };
            copyMode = lib.mkOption {
              type = fileCopyModeType;
              default = "symlink";
              description = ''
                How to materialise the skill's files, as in `files.<name>.copyMode`.
                Use `seed` to hand-edit a skill after devenv writes it once.
              '';
            };
            assertions = lib.mkOption {
              type = lib.types.listOf lib.types.unspecified;
              default = [ ];
              internal = true;
              visible = false;
              description = "Assertions raised by this skill's submodule, collected into the top-level assertions.";
            };
            warnings = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              internal = true;
              visible = false;
              description = "Warnings raised by this skill's submodule, collected into the top-level warnings.";
            };
          };

          config.warnings =
            let
              hasWhenToUse = config.whenToUse != null;
              listingLength =
                builtins.stringLength config.description
                + (if hasWhenToUse then builtins.stringLength config.whenToUse else 0);
              subject =
                if hasWhenToUse
                then "`description` and `whenToUse` add up to"
                else "`description` is";
            in
            lib.optional (listingLength > skillListingMaxDescChars) ''
              claude.code.skills.${name}: ${subject} ${toString listingLength} characters, over the
              ${toString skillListingMaxDescChars} Claude Code keeps in the skill listing. The rest is truncated, so
              put the key use case first or move the detail into `content`.
            '';

          config.assertions = [
            {
              assertion = builtins.match "[a-z0-9]+(-[a-z0-9]+)*" name != null && builtins.stringLength name <= 64;
              message = ''
                claude.code.skills.${name}: skill names must be lowercase letters, digits and
                single hyphens, at most 64 characters. Claude Code skips directories it cannot
                match to a valid skill name.
              '';
            }
            {
              assertion = name != "synced";
              message = ''
                claude.code.skills.synced: `synced` is a reserved directory name. Claude Code
                uses .claude/skills/synced/ for the skills it downloads from your claude.ai
                account, and skips a skill you author at that name. Pick a different name.
              '';
            }
          ];
        })
      );
      default = { };
      description = ''
        Custom Claude Code skills to create in the project.
        A skill is a folder of instructions Claude loads on demand when its
        description matches the task, keeping specialised knowledge out of the
        always-on context.

        For more details, see: https://docs.anthropic.com/en/docs/claude-code/skills
      '';
      example = lib.literalExpression ''
        {
          database-migrations = {
            description = "How to write and run migrations in this project. Use when adding, editing or rolling back a migration.";
            content = '''
              Migrations live in `migrations/` and are applied with `diesel migration run`.

              - One logical change per migration; never edit an applied migration.
              - Always write the matching `down.sql`.
              - Run `devenv tasks run db:reset` before opening a pull request.
            ''';
          };

          api-conventions = {
            description = "REST conventions for this codebase. Use when adding or changing an HTTP endpoint.";
            allowedTools = [ "Read" "Grep" ];
            resources = {
              "references/error-codes.md" = ./docs/error-codes.md;
            };
            content = '''
              Endpoints are versioned under `/v1`.
              Read references/error-codes.md before inventing a new error code.
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
          "claudeai"
          "console"
          "gateway"
        ]
      );
      default = null;
      description = ''
        Restrict the login method.
        - claudeai: only claude.ai accounts
        - console: only Claude Console (API key) accounts
        - gateway: only a cloud gateway
      '';
      example = "claudeai";
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
                "manual"
                "acceptEdits"
                "plan"
                "auto"
                "dontAsk"
                "bypassPermissions"
              ]
            );
            default = null;
            description = ''
              Global permission mode for Claude Code.
              - default: Prompts on first use of each tool (`manual` is an alias)
              - acceptEdits: Auto-accepts file edits
              - plan: Read-only mode
              - auto: Auto-approves tool calls with background safety checks
              - dontAsk: Auto-denies unless pre-approved via permissions
              - bypassPermissions: Skips all permission prompts
            '';
            example = "acceptEdits";
          };
          disableBypassPermissionsMode = lib.mkOption {
            type = lib.types.nullOr lib.types.bool;
            default = null;
            description = ''
              Security option to prevent the dangerous bypassPermissions mode.
              Written to `settings.json` as `"disableBypassPermissionsMode": "disable"`.
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
            # Use an empty string to emit a bare tool entry for tools
            # without a matcher format (e.g. WebSearch, AskUserQuestion).
            WebSearch.allow = [ "" ];
            AskUserQuestion.deny = [ "" ];
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
  };

  config = lib.mkMerge [
    {
      changelogs = [
        {
          date = "2026-03-10";
          title = "claude.code.hooks.git-hooks-format renamed to git-hooks-run";
          when = cfg.enable;
          description = ''
            The `claude.code.hooks.git-hooks-format` hook has been renamed to `claude.code.hooks.git-hooks-run`.
          '';
        }
        {
          date = "2026-08-16";
          title = "claude.code.forceLoginMethod values changed to match Claude Code";
          when = cfg.enable;
          description = ''
            `claude.code.forceLoginMethod` now accepts `claudeai`, `console`, or `gateway`, the values Claude Code recognises.
            The previous `browser` and `api-key` values were never valid; replace them with `claudeai` and `console` respectively.
          '';
        }
        {
          date = "2026-08-16";
          title = "claude.code.agents.<name>.proactive removed";
          when = cfg.enable;
          description = ''
            `claude.code.agents.<name>.proactive` has been removed and setting it is now an error.
            Claude Code has no `proactive` frontmatter field, so the option never had an effect.
            To encourage automatic delegation, include a phrase like "use proactively" in the agent's `description`.
          '';
        }
      ];
    }

    {
      assertions = lib.flatten (lib.mapAttrsToList (_: agent: agent.assertions) cfg.agents)
        ++ lib.flatten (lib.mapAttrsToList (_: skill: skill.assertions) cfg.skills);
      warnings = lib.flatten (lib.mapAttrsToList (_: skill: skill.warnings) cfg.skills);
    }

    (lib.mkIf cfg.enable {
      files = lib.mkMerge [
        { "${cfg.settingsPath}".json = settingsContent; }

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
                ${lib.optionalString (agent.tools != []) "tools:\n${lib.concatMapStringsSep "\n" (tool: "  - ${tool}") agent.tools}"}
                ${lib.optionalString (agent.model != null) "model: ${agent.model}"}
                ${lib.optionalString (agent.effort != null) "effort: ${agent.effort}"}
                ${lib.optionalString (agent.permissionMode != null) "permissionMode: ${agent.permissionMode}"}
                ---

                ${agent.prompt}
              '';
            };
          })
          cfg.agents)

        # Skill files
        (lib.mapAttrs'
          (name: skill: {
            name = ".claude/skills/${name}/SKILL.md";
            value = {
              inherit (skill) copyMode;
              text =
                let
                  # toJSON keeps colons and quotes valid; a list renders as a JSON
                  # array, sidestepping the comma/space separator rules these fields
                  # disagree on.
                  scalar = key: value: "${key}: ${builtins.toJSON value}";
                  optionalScalar = key: value: lib.optional (value != null) (scalar key value);
                  optionalList = key: value: lib.optional (value != [ ]) (scalar key value);
                  optionalFlag = key: value: default: lib.optional (value != default) "${key}: ${lib.boolToString value}";

                  # Built as a list so an unset field leaves no blank line behind.
                  frontmatter = [
                    "name: ${name}"
                    (scalar "description" skill.description)
                  ]
                  ++ optionalScalar "when_to_use" skill.whenToUse
                  ++ optionalList "allowed-tools" skill.allowedTools
                  ++ optionalList "disallowed-tools" skill.disallowedTools
                  ++ optionalFlag "disable-model-invocation" skill.disableModelInvocation false
                  ++ optionalFlag "user-invocable" skill.userInvocable true
                  ++ optionalScalar "argument-hint" skill.argumentHint
                  ++ optionalList "arguments" skill.arguments
                  ++ optionalScalar "model" skill.model
                  ++ optionalScalar "effort" skill.effort
                  ++ optionalScalar "context" skill.context
                  ++ optionalScalar "agent" skill.agent
                  ++ lib.optional (skill.background != null)
                    "background: ${lib.boolToString skill.background}";
                in
                ''
                  ---
                  ${lib.concatStringsSep "\n" frontmatter}
                  ---

                  ${skill.content}
                '';
            };
          })
          cfg.skills)

        # Files bundled alongside a skill (references, scripts, assets)
        (lib.listToAttrs (lib.concatLists (lib.mapAttrsToList
          (name: skill: lib.mapAttrsToList
            (path: resource: lib.nameValuePair ".claude/skills/${name}/${path}" {
              source = resource.file;
              inherit (skill) copyMode;
            })
            skill.resources)
          cfg.skills)))
      ];

      # Add a message about the integration
      infoSections."claude" =
        let
          primaryAgents = lib.filterAttrs (n: a: n == cfg.agent) cfg.agents;
          primaryAgent = if primaryAgents != { } then builtins.head (lib.attrNames primaryAgents) else cfg.agent;
          subAgents = lib.filterAttrs (n: a: n != cfg.agent) cfg.agents;
        in
        [
          ''
            Claude Code integration is enabled with automatic hooks and commands setup.
            Settings are configured at: ${cfg.settingsPath}
            ${lib.optionalString config.git-hooks.enable "- Auto-formatting: enabled via git-hooks (git-hooks-run)"}
            ${lib.optionalString (cfg.commands != { })
              "- Project commands: ${
                lib.concatStringsSep ", " (map (cmd: "/${cmd}") (lib.attrNames cfg.commands))
              }"
            }
            ${lib.optionalString (primaryAgent != null)
              "- Primary agent: ${primaryAgent}"
            }
            ${lib.optionalString (subAgents != { })
              "- Sub-agents: ${
                lib.concatStringsSep ", " (lib.attrNames subAgents)
              }"
            }
            ${lib.optionalString (cfg.skills != { })
              "- Skills: ${
                lib.concatStringsSep ", " (lib.attrNames cfg.skills)
              }"
            }
            ${lib.optionalString (cfg.mcpServers != { })
              "- MCP servers: ${
                lib.concatStringsSep ", " (lib.attrNames cfg.mcpServers)
              } (configured at ${config.devenv.root}/.mcp.json)"
            }
          ''
        ];
    })

  ];
}
