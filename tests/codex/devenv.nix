{ pkgs, ... }:

{
  codex = {
    enable = true;

    settings = {
      model = "gpt-5.4";
      sandbox_mode = "workspace-write";
      agents.max_concurrent_threads_per_session = 4;

      # Native Codex hooks remain available through the free-form settings.
      hooks.Stop = [
        {
          hooks = [
            {
              type = "command";
              command = "echo started";
            }
          ];
        }
      ];

      # This should override codex.mcpServers.docs.
      mcp_servers.docs = {
        url = "https://override.example.com";
      };
    };

    hooks.load-context = {
      hookType = "SessionStart";
      matcher = "startup|resume";
      command = "echo context";
      timeout = 30;
      statusMessage = "Loading context";
      additionalContextLimit = 5000;
    };

    mcpServers = {
      local = {
        type = "stdio";
        command = "devenv";
        args = [ "mcp" ];
        env.DEVENV_ROOT = "test-root";
      };

      docs = {
        type = "http";
        url = "https://mcp.example.com";
        settings.bearer_token_env_var = "MCP_TOKEN";
      };
    };

    agents.reviewer = {
      description = "Review changes for correctness and regressions.";
      prompt = "Report actionable findings first.";
      model = "gpt-6.1-sol";
      effort = "high";
      settings.features.shell_tool = false;
    };

    skills = {
      inline = {
        description = "Test an inline skill.";
        content = "INLINE SKILL CONTENT";
        resources."references/testing.md" = ./fixtures/testing.md;
      };
      directory = ./fixtures/directory-skill;
    };
  };

  enterTest = ''
    echo "=== Testing Codex Configuration ==="

    ${pkgs.python3}/bin/python - <<'PY'
    import pathlib
    import tomllib

    config = tomllib.loads(pathlib.Path(".codex/config.toml").read_text())
    assert config["model"] == "gpt-5.4"
    assert config["sandbox_mode"] == "workspace-write"
    assert config["mcp_servers"]["local"]["command"] == "devenv"
    assert config["mcp_servers"]["local"]["args"] == ["mcp"]
    assert config["mcp_servers"]["local"]["env"]["DEVENV_ROOT"] == "test-root"
    assert config["mcp_servers"]["docs"]["url"] == "https://override.example.com"
    assert config["agents"]["max_concurrent_threads_per_session"] == 4
    assert config["hooks"]["SessionStart"][0]["matcher"] == "startup|resume"
    session_hook = config["hooks"]["SessionStart"][0]["hooks"][0]
    assert session_hook["command"] == "echo context"
    assert session_hook["timeout"] == 30
    assert session_hook["statusMessage"] == "Loading context"
    assert session_hook["additionalContextLimit"] == 5000
    assert config["hooks"]["Stop"][0]["hooks"][0]["command"] == "echo started"

    reviewer = tomllib.loads(pathlib.Path(".codex/agents/reviewer.toml").read_text())
    assert reviewer["name"] == "reviewer"
    assert reviewer["description"] == "Review changes for correctness and regressions."
    assert reviewer["developer_instructions"] == "Report actionable findings first."
    assert reviewer["model"] == "gpt-6.1-sol"
    assert reviewer["model_reasoning_effort"] == "high"
    assert reviewer["features"]["shell_tool"] is False
    PY

    test -f .agents/skills/inline/SKILL.md
    grep -q "INLINE SKILL CONTENT" .agents/skills/inline/SKILL.md
    test -f .agents/skills/inline/references/testing.md
    grep -q "TESTING RESOURCE" .agents/skills/inline/references/testing.md
    test -f .agents/skills/directory/SKILL.md
    grep -q "DIRECTORY SKILL CONTENT" .agents/skills/directory/SKILL.md

    echo "=== All Codex tests passed! ==="
  '';
}
