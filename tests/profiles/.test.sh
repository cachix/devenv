set -e

# Helper function to check JSON output
check_env_var() {
  local json_output="$1"
  local var_name="$2"
  local expected_value="$3"

  if echo "$json_output" | jq -e ".variables.\"$var_name\".value == \"$expected_value\"" >/dev/null; then
    return 0
  else
    echo "Expected $var_name=$expected_value, got:"
    echo "$json_output" | jq -r ".variables.\"$var_name\" // \"(not found)\""
    return 1
  fi
}

# Helper function to check if package is in PATH
check_package_in_path() {
  local json_output="$1"
  local package_name="$2"

  if echo "$json_output" | jq -e ".variables.PATH.value | contains(\"$package_name\")" >/dev/null; then
    return 0
  else
    echo "Package $package_name not found in PATH"
    return 1
  fi
}

# Helper function to check if package exists in packages list
check_package_exists() {
  local json_output="$1"
  local package_name="$2"

  # Check if package name appears anywhere in the packages or PATH
  if echo "$json_output" | jq -e "(.variables.PATH.value // \"\") | contains(\"$package_name\")" >/dev/null; then
    return 0
  else
    echo "Package $package_name not found"
    return 1
  fi
}

# Test 1: Base configuration (no profiles)
echo "Test 1: Base configuration (no profiles)"
json_output=$(devenv print-dev-env --json)
if check_env_var "$json_output" "BASE_ENV" "base-value"; then
  echo "✓ Base environment variable found"
else
  echo "✗ Base environment variable not found"
  exit 1
fi

# Test 2: Single profile (basic)
echo "Test 2: Basic profile"
json_output=$(devenv --profile basic print-dev-env --json)
if check_env_var "$json_output" "BASIC_PROFILE" "enabled" && check_package_exists "$json_output" "curl"; then
  echo "✓ Basic profile active with correct env var and package"
else
  echo "✗ Basic profile not working"
  exit 1
fi

# Test 3: Backend profile
echo "Test 3: Backend profile"
json_output=$(devenv --profile backend print-dev-env --json)
if check_env_var "$json_output" "BACKEND_ENABLED" "true" && check_package_exists "$json_output" "wget" && check_package_exists "$json_output" "tree"; then
  echo "✓ Backend profile enabled with correct packages"
else
  echo "✗ Backend profile not working"
  exit 1
fi

# Test 4: Multiple profiles (backend + extra-packages) - Tests package merging
echo "Test 4: Multiple profiles (backend + extra-packages) - Package merging"
json_output=$(devenv --profile backend --profile extra-packages print-dev-env --json)
if check_env_var "$json_output" "BACKEND_ENABLED" "true" &&
  check_env_var "$json_output" "EXTRA_TOOLS" "enabled" &&
  check_package_exists "$json_output" "wget" &&
  check_package_exists "$json_output" "tree" &&
  check_package_exists "$json_output" "jq" &&
  check_package_exists "$json_output" "htop"; then
  echo "✓ Multiple profiles merged correctly with all packages"
else
  echo "✗ Multiple profiles not working - packages not properly merged"
  exit 1
fi

# Test 5: Multiple profiles with priority handling and package merging (profile-a + profile-b)
echo "Test 5: Multiple profiles - env priority + package merging (both have curl)"
json_output=$(devenv --profile profile-a --profile profile-b print-dev-env --json)
if check_env_var "$json_output" "MERGE_TEST" "profile-b" &&
  check_env_var "$json_output" "PROFILE_A" "active" &&
  check_env_var "$json_output" "PROFILE_B" "active" &&
  check_package_exists "$json_output" "curl" &&
  check_package_exists "$json_output" "wget" &&
  check_package_exists "$json_output" "jq" &&
  check_package_exists "$json_output" "tree"; then
  echo "✓ Env vars use priority (mkForce > mkDefault), packages merge (including overlapping curl)"
else
  echo "✗ Profile priority not working correctly or packages not merged"
  exit 1
fi

# Test 6: Profile extends - single inheritance
echo "Test 6: Profile extends - single inheritance (child-profile extends base-profile)"
json_output=$(devenv --profile child-profile print-dev-env --json)
if check_env_var "$json_output" "BASE_PROFILE" "enabled" &&
  check_env_var "$json_output" "CHILD_PROFILE" "enabled" &&
  check_env_var "$json_output" "EXTENDS_TEST" "child" &&
  check_package_exists "$json_output" "git" &&
  check_package_exists "$json_output" "curl" &&
  check_package_exists "$json_output" "wget"; then
  echo "✓ Single profile extends working correctly with inherited packages"
else
  echo "✗ Single profile extends failed"
  exit 1
fi

# Test 7: Profile extends - nested inheritance
echo "Test 7: Profile extends - nested inheritance (grandchild-profile extends child-profile extends base-profile)"
json_output=$(devenv --profile grandchild-profile print-dev-env --json)
if check_env_var "$json_output" "BASE_PROFILE" "enabled" &&
  check_env_var "$json_output" "CHILD_PROFILE" "enabled" &&
  check_env_var "$json_output" "GRANDCHILD_PROFILE" "enabled" &&
  check_env_var "$json_output" "EXTENDS_TEST" "grandchild" &&
  check_package_exists "$json_output" "git" &&
  check_package_exists "$json_output" "curl" &&
  check_package_exists "$json_output" "wget" &&
  check_package_exists "$json_output" "tree"; then
  echo "✓ Nested profile extends working correctly with all inherited packages"
else
  echo "✗ Nested profile extends failed"
  exit 1
fi

# Test 8: Profile extends - multiple inheritance
echo "Test 8: Profile extends - multiple inheritance (multiple-extends extends basic and backend)"
json_output=$(devenv --profile multiple-extends print-dev-env --json)
if check_env_var "$json_output" "BASIC_PROFILE" "enabled" &&
  check_env_var "$json_output" "BACKEND_ENABLED" "true" &&
  check_env_var "$json_output" "MULTIPLE_EXTENDS" "enabled" &&
  check_package_exists "$json_output" "curl" &&
  check_package_exists "$json_output" "wget" &&
  check_package_exists "$json_output" "tree" &&
  check_package_exists "$json_output" "htop"; then
  echo "✓ Multiple profile extends working correctly with all packages"
else
  echo "✗ Multiple profile extends failed"
  exit 1
fi

# Test 9: Profile priority conflicts (last one wins)
echo "Test 9: Profile priority conflicts - last profile should win"
json_output=$(devenv --profile conflict-low --profile conflict-middle --profile conflict-high print-dev-env --json)
if check_env_var "$json_output" "CONFLICT_VAR" "high-priority" &&
  check_env_var "$json_output" "CONFLICT_LOW" "enabled" &&
  check_env_var "$json_output" "CONFLICT_MIDDLE" "enabled" &&
  check_env_var "$json_output" "CONFLICT_HIGH" "enabled"; then
  echo "✓ Profile priority conflicts working correctly (last one wins)"
else
  echo "✗ Profile priority conflicts failed"
  exit 1
fi

# Test 10: Circular dependency detection
echo "Test 10: Circular dependency detection (cycle-a extends cycle-b extends cycle-a)"
error_output=$(devenv --profile cycle-a info 2>&1 || true)
if echo "$error_output" | grep -q "Circular dependency detected"; then
  echo "✓ Circular dependency detection working correctly"
else
  echo "✗ Circular dependency detection failed: $error_output"
  exit 1
fi

# Test 11: Profile precedence with both functions
echo "Test 11: Profile precedence with both functions"
json_output=$(devenv --profile function-profile --profile attrset-profile print-dev-env --json)
if check_env_var "$json_output" "TEST_VAR" "foobar" && check_env_var "$json_output" "BASE_ENV" "foobar"; then
  echo "✓ Profile priorities working correctly - attrset-profile overrides function-profile, profiles override base config"
else
  echo "✗ Profile precedence failed"
  exit 1
fi

# Test 12: Package merging from base + profiles
echo "Test 12: Package merging from base configuration + profiles"
json_output=$(devenv --profile basic print-dev-env --json)
if check_package_exists "$json_output" "git" &&
  check_package_exists "$json_output" "hello" &&
  check_package_exists "$json_output" "curl"; then
  echo "✓ Base packages and profile packages properly merged"
else
  echo "✗ Package merging failed - base packages or profile packages missing"
  exit 1
fi

# Test 13: Comprehensive mixed-type merging validation
echo "Test 13: Comprehensive validation - profile priority order, packages merge"
json_output=$(devenv --profile conflict-low --profile conflict-middle --profile conflict-high print-dev-env --json)
# conflict-high should win (last profile wins), all env vars should be present
if check_env_var "$json_output" "CONFLICT_VAR" "high-priority" &&
  check_env_var "$json_output" "CONFLICT_LOW" "enabled" &&
  check_env_var "$json_output" "CONFLICT_MIDDLE" "enabled" &&
  check_env_var "$json_output" "CONFLICT_HIGH" "enabled"; then
  echo "✓ Mixed types: env vars resolved by profile order, all profile-specific vars present"
else
  echo "✗ Mixed type merging validation failed"
  exit 1
fi

# Test 14: enterShell concatenation (types.lines should merge, not conflict)
echo "Test 14: enterShell concatenation - types.lines should append"
json_output=$(devenv --profile shell-setup-a --profile shell-setup-b print-dev-env --json)
# Both enterShell scripts should be present (concatenated)
shellHook=$(echo "$json_output" | jq -r '.variables.shellHook.value // ""')
if check_env_var "$json_output" "SHELL_SETUP_A" "enabled" &&
  check_env_var "$json_output" "SHELL_SETUP_B" "enabled" &&
  check_package_exists "$json_output" "curl" &&
  check_package_exists "$json_output" "wget" &&
  echo "$shellHook" | grep -q "profile A" &&
  echo "$shellHook" | grep -q "profile B" &&
  echo "$shellHook" | grep -q "SHELL_A" &&
  echo "$shellHook" | grep -q "SHELL_B"; then
  echo "✓ enterShell (types.lines) concatenates correctly, env vars still use priority"
else
  echo "✗ enterShell concatenation failed"
  echo "shellHook content:"
  echo "$shellHook"
  exit 1
fi

# Test 15: CLI parsing - profile without subcommand (issue #2206)
echo "Test 15: CLI parsing - profile without subcommand"
version_output=$(devenv --profile basic 2>&1)
if echo "$version_output" | grep -q "devenv.*(.*)"; then
  echo "✓ Profile flag parses correctly without subcommand (shows version)"
else
  echo "✗ Profile flag parsing failed without subcommand: $version_output"
  exit 1
fi

# Test 16: CLI parsing - profile with --help flag (issue #2206)
echo "Test 16: CLI parsing - profile with --help flag"
help_output=$(devenv --profile basic --help 2>&1)
if echo "$help_output" | grep -q "Usage: devenv"; then
  echo "✓ Help displays correctly with --profile flag"
else
  echo "✗ Help not working with --profile flag"
  exit 1
fi

# Test 17: CLI parsing - profile with subcommand and --help (issue #2206)
echo "Test 17: CLI parsing - profile with subcommand and --help"
help_output=$(devenv --profile basic test --help 2>&1)
if echo "$help_output" | grep -q "Run tests" && echo "$help_output" | grep -q "Usage: devenv test"; then
  echo "✓ Subcommand help displays correctly with --profile flag"
else
  echo "✗ Subcommand help not working with --profile flag: $help_output"
  exit 1
fi

# Test 18: CLI parsing - multiple profiles with --help (issue #2206)
echo "Test 18: CLI parsing - multiple profiles with --help"
help_output=$(devenv --profile basic --profile backend --help 2>&1)
if echo "$help_output" | grep -q "Usage: devenv"; then
  echo "✓ Help displays correctly with multiple --profile flags"
else
  echo "✗ Help not working with multiple --profile flags"
  exit 1
fi

echo "All profile tests passed!"
