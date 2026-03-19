#!/usr/bin/env bash
set -euo pipefail

echo "Running additional OpenCode validation tests..."

# Function to assert file exists and contains expected content
assert_file_contains() {
  local file=$1
  local pattern=$2

  if [ ! -f "$file" ]; then
    echo "❌ File not found: $file"
    exit 1
  fi

  if ! grep -q "$pattern" "$file"; then
    echo "❌ Pattern not found in $file: $pattern"
    echo "File contents:"
    cat "$file"
    exit 1
  fi
}

echo "=== Testing file content integrity ==="

# Test opencode.jsonc structure
echo "Validating opencode.jsonc structure..."
assert_file_contains "opencode.jsonc" '"$schema"'
assert_file_contains "opencode.jsonc" '"editor"'
assert_file_contains "opencode.jsonc" '"theme"'
assert_file_contains "opencode.jsonc" '"mcp"'
assert_file_contains "opencode.jsonc" '"local-dev"'

# Test that external file content matches source exactly
echo "Validating directory command file matches source..."
diff -u fixtures/commands-dir/test-command.md .opencode/commands/test-command.md || {
  echo "❌ Directory command file doesn't match source"
  exit 1
}

echo "Validating directory agent file matches source..."
diff -u fixtures/agents-dir/test-agent.md .opencode/agents/test-agent.md || {
  echo "❌ Directory agent file doesn't match source"
  exit 1
}

echo "Validating simple skill file matches source..."
diff -u fixtures/skills-dir/alpha/SKILL.md .opencode/skills/alpha/SKILL.md || {
  echo "❌ Directory skill file doesn't match source"
  exit 1
}

# Test tools directory mode
echo "Validating tools directory mode..."
diff -u fixtures/tools-dir/sample.ts .opencode/tools/sample.ts || {
  echo "❌ Tool file doesn't match source"
  exit 1
}

# Test themes attrs/path mode
echo "Validating themes generation..."
diff -u fixtures/themes-dir/base.json .opencode/themes/base.json || {
  echo "❌ Base theme file doesn't match source"
  exit 1
}
assert_file_contains ".opencode/themes/generated-theme.json" '"$schema"'
assert_file_contains ".opencode/themes/generated-theme.json" '"#112233"'

# Verify directory structure is correct
echo "Validating directory structure..."
test -d .opencode/commands || { echo "❌ commands directory missing"; exit 1; }
test -d .opencode/agents || { echo "❌ agents directory missing"; exit 1; }
test -d .opencode/skills || { echo "❌ skills directory missing"; exit 1; }
test -d .opencode/tools || { echo "❌ tools directory missing"; exit 1; }
test -d .opencode/themes || { echo "❌ themes directory missing"; exit 1; }

# Count files to ensure nothing extra was created
echo "Validating file counts..."
commands_count=$(find .opencode/commands \( -type f -o -type l \) | wc -l | tr -d ' ')
agents_count=$(find .opencode/agents \( -type f -o -type l \) | wc -l | tr -d ' ')
if [ -L .opencode/skills ]; then
  skills_count=1
else
  skills_count=$(find .opencode/skills -mindepth 1 -maxdepth 1 \( -type d -o -type l \) | wc -l | tr -d ' ')
fi
tools_count=$(find .opencode/tools \( -type f -o -type l \) | wc -l | tr -d ' ')
themes_count=$(find .opencode/themes \( -type f -o -type l \) | wc -l | tr -d ' ')

test "$commands_count" -eq 1 || { echo "❌ Expected 1 command, found $commands_count"; exit 1; }
test "$agents_count" -eq 1 || { echo "❌ Expected 1 agent, found $agents_count"; exit 1; }
test "$skills_count" -eq 1 || { echo "❌ Expected 1 skill, found $skills_count"; exit 1; }
test "$tools_count" -eq 1 || { echo "❌ Expected 1 tool, found $tools_count"; exit 1; }
test "$themes_count" -eq 2 || { echo "❌ Expected 2 themes, found $themes_count"; exit 1; }

echo ""
echo "✅ All OpenCode file validation tests passed!"
