{ pkgs, config, ... }:
{
  opencode = {
    enable = true;

    # Test custom settings
    settings = {
      editor = "nvim";
      theme = "dark";
      features = {
        autocomplete = true;
        git_integration = true;
      };

      # This should override opencode.mcp.docs
      mcp.docs = {
        enabled = true;
        type = "remote";
        url = "https://override.example.com";
      };
    };

    # Test inline rules
    rules = ''
      # Custom Development Rules
      - Always write tests
      - Use conventional commits
    '';

    # Test commands and agents from directories
    commands = ./fixtures/commands-dir;
    agents = ./fixtures/agents-dir;

    # Test skills directory mode
    skills = ./fixtures/skills-dir;

    # Test tools directory mode
    tools = ./fixtures/tools-dir;

    # Test themes attrs mode (including path source)
    themes = {
      "generated-theme" = {
        theme = {
          primary = "#112233";
        };
      };

      base = ./fixtures/themes-dir/base.json;
    };

    web.enable = false;

    # Test MCP configuration and merge precedence
    mcp = {
      local-dev = {
        type = "local";
        enabled = true;
        command = [
          "devenv"
          "mcp"
        ];
        environment = {
          DEVENV_ROOT = "test-root";
        };
      };

      docs = {
        type = "remote";
        url = "https://mcp.example.com";
        headers = {
          Authorization = "Bearer test-token";
        };
      };
    };
  };

  # Test verification
  enterTest = ''
    echo "=== Testing OpenCode Configuration ==="

    # Test 1: opencode.jsonc exists and has correct content
    echo "Testing opencode.jsonc..."
    test -f opencode.jsonc || { echo "❌ opencode.jsonc not found"; exit 1; }
    ${pkgs.jq}/bin/jq -e '."$schema" == "https://opencode.ai/config.json"' opencode.jsonc > /dev/null || { echo "❌ Schema incorrect"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.editor == "nvim"' opencode.jsonc > /dev/null || { echo "❌ Custom settings not applied"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.theme == "dark"' opencode.jsonc > /dev/null || { echo "❌ Theme setting not applied"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.features.autocomplete == true' opencode.jsonc > /dev/null || { echo "❌ Feature settings not applied"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.mcp."local-dev".enabled == true' opencode.jsonc > /dev/null || { echo "❌ MCP enabled flag missing"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.mcp."local-dev".type == "local"' opencode.jsonc > /dev/null || { echo "❌ MCP local type wrong"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.mcp."local-dev".command[0] == "devenv"' opencode.jsonc > /dev/null || { echo "❌ MCP local command wrong"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.mcp."local-dev".environment.DEVENV_ROOT == "test-root"' opencode.jsonc > /dev/null || { echo "❌ MCP local environment wrong"; exit 1; }
    ${pkgs.jq}/bin/jq -e '.mcp.docs.url == "https://override.example.com"' opencode.jsonc > /dev/null || { echo "❌ settings.mcp did not override opencode.mcp"; exit 1; }
    echo "✅ opencode.jsonc OK"

    # Test 2: AGENTS.md exists and has content
    echo "Testing AGENTS.md..."
    test -f .opencode/AGENTS.md || { echo "❌ AGENTS.md not found"; exit 1; }
    grep -q "Always write tests" .opencode/AGENTS.md || { echo "❌ Rules content incorrect"; exit 1; }
    grep -q "Use conventional commits" .opencode/AGENTS.md || { echo "❌ Rules content incomplete"; exit 1; }
    echo "✅ AGENTS.md OK"

    # Test 3: Commands created
    echo "Testing commands..."
    test -f .opencode/commands/test-command.md || { echo "❌ Directory command not found"; exit 1; }
    grep -q "EXTERNAL COMMAND CONTENT" .opencode/commands/test-command.md || { echo "❌ Directory command content wrong"; exit 1; }
    echo "✅ Commands OK"

    # Test 4: Agents created
    echo "Testing agents..."
    test -f .opencode/agents/test-agent.md || { echo "❌ Directory agent not found"; exit 1; }
    grep -q "EXTERNAL AGENT CONTENT" .opencode/agents/test-agent.md || { echo "❌ Directory agent content wrong"; exit 1; }
    echo "✅ Agents OK"

    # Test 5: Skills created (directory mode)
    echo "Testing skills..."
    test -f .opencode/skills/alpha/SKILL.md || { echo "❌ Directory skill not found"; exit 1; }
    grep -q "SIMPLE SKILL CONTENT" .opencode/skills/alpha/SKILL.md || { echo "❌ Directory skill content wrong"; exit 1; }
    echo "✅ Skills OK"

    # Test 6: Tools created
    echo "Testing tools..."
    test -f .opencode/tools/sample.ts || { echo "❌ Tool file not found"; exit 1; }
    grep -q "sample-tool" .opencode/tools/sample.ts || { echo "❌ Tool content wrong"; exit 1; }
    echo "✅ Tools OK"

    # Test 7: Themes created
    echo "Testing themes..."
    test -f .opencode/themes/base.json || { echo "❌ Path theme not found"; exit 1; }
    test -f .opencode/themes/generated-theme.json || { echo "❌ Generated theme not found"; exit 1; }
    grep -q '"$schema": "https://opencode.ai/theme.json"' .opencode/themes/generated-theme.json || { echo "❌ Generated theme schema missing"; exit 1; }
    grep -q '"primary": "#112233"' .opencode/themes/generated-theme.json || { echo "❌ Generated theme content wrong"; exit 1; }
    echo "✅ Themes OK"

    # Test 8: State tracking
    echo "Testing state tracking..."
    test -f .devenv/state/files.json || { echo "❌ State tracking not working"; exit 1; }
    echo "✅ State tracking OK"

    # Test 9: Verify minimal config works (edge case)
    echo "Testing minimal configuration support..."
    # At minimum, opencode.jsonc should always be created when enabled
    test -f opencode.jsonc || { echo "❌ Minimal config didn't create opencode.jsonc"; exit 1; }
    echo "✅ Minimal config OK"

    echo ""
    echo "=== All OpenCode tests passed! ==="
  '';
}
