set -e

# # Test profile error handling with no profiles defined
# echo "Testing profile error handling..."
# error_output=$(devenv --profile some-profile info 2>&1 || true)
# if echo "$error_output" | grep -q "Profile 'some-profile' not found"; then
#     echo "✓ Profile error handling works correctly"
# else
#     echo "✗ Profile error handling failed: $error_output"
#     exit 1
# fi
#
# # Test 1: No profiles (base configuration)
# echo "Test 1: Base configuration (no profiles)"
# if devenv print-dev-env | grep -q 'BASE_ENV.*base-value'; then
#     echo "✓ Base environment variable found"
# else
#     echo "✗ Base environment variable not found"
#     exit 1
# fi

# Test 2: Single profile (basic)
echo "Test 2: Basic profile"
if devenv --profile basic print-dev-env | grep -q 'BASIC_PROFILE.*enabled'; then
  echo "✓ Basic profile active"
else
  echo "✗ Basic profile not working"
  exit 1
fi

# Test 3: Backend profile
echo "Test 3: Backend profile"
if devenv --profile backend print-dev-env | grep -q 'BACKEND_ENABLED.*true'; then
  echo "✓ Backend profile enabled"
else
  echo "✗ Backend profile not working"
  exit 1
fi

# Test 4: Multiple profiles (backend + extra-packages)
echo "Test 4: Multiple profiles (backend + extra-packages)"
output=$(devenv --profile backend --profile extra-packages print-dev-env)
if echo "$output" | grep -q 'BACKEND_ENABLED.*true' && echo "$output" | grep -q 'EXTRA_TOOLS.*enabled'; then
  echo "✓ Multiple profiles merged correctly"
else
  echo "✗ Multiple profiles not working"
  exit 1
fi

# Profile merging tests - all in same directory now

# Test 5: Multiple profiles with priority handling
echo "Test 5: Multiple profiles with priority handling"
output=$(devenv --profile profile-a --profile profile-b print-dev-env)
if echo "$output" | grep -q 'MERGE_TEST.*profile-b' && echo "$output" | grep -q 'PROFILE_A.*active' && echo "$output" | grep -q 'PROFILE_B.*active'; then
  echo "✓ Multiple profiles merged with correct priority (mkForce > mkDefault)"
else
  echo "✗ Profile priority not working correctly"
  exit 1
fi

# Test 6: Profile extends - single inheritance
echo "Test 6: Profile extends - single inheritance (child-profile extends base-profile)"
output=$(devenv --profile child-profile print-dev-env)
if echo "$output" | grep -q 'BASE_PROFILE.*enabled' && echo "$output" | grep -q 'CHILD_PROFILE.*enabled' && echo "$output" | grep -q 'EXTENDS_TEST.*child'; then
  echo "✓ Single profile extends working correctly"
else
  echo "✗ Single profile extends failed"
  exit 1
fi

# Test 7: Profile extends - nested inheritance
echo "Test 7: Profile extends - nested inheritance (grandchild-profile extends child-profile extends base-profile)"
output=$(devenv --profile grandchild-profile print-dev-env)
if echo "$output" | grep -q 'BASE_PROFILE.*enabled' && echo "$output" | grep -q 'CHILD_PROFILE.*enabled' && echo "$output" | grep -q 'GRANDCHILD_PROFILE.*enabled' && echo "$output" | grep -q 'EXTENDS_TEST.*grandchild'; then
  echo "✓ Nested profile extends working correctly"
else
  echo "✗ Nested profile extends failed"
  exit 1
fi

# Test 8: Profile extends - multiple inheritance
echo "Test 8: Profile extends - multiple inheritance (multiple-extends extends basic and backend)"
output=$(devenv --profile multiple-extends print-dev-env)
if echo "$output" | grep -q 'BASIC_PROFILE.*enabled' && echo "$output" | grep -q 'BACKEND_ENABLED.*true' && echo "$output" | grep -q 'MULTIPLE_EXTENDS.*enabled'; then
  echo "✓ Multiple profile extends working correctly"
else
  echo "✗ Multiple profile extends failed"
  exit 1
fi

# Test 9: Profile priority conflicts (last one wins)
echo "Test 9: Profile priority conflicts - last profile should win"
output=$(devenv --profile conflict-low --profile conflict-middle --profile conflict-high print-dev-env)
if echo "$output" | grep -q 'CONFLICT_VAR.*high-priority' && echo "$output" | grep -q 'CONFLICT_LOW.*enabled' && echo "$output" | grep -q 'CONFLICT_MIDDLE.*enabled' && echo "$output" | grep -q 'CONFLICT_HIGH.*enabled'; then
  echo "✓ Profile priority conflicts working correctly (last one wins)"
else
  echo "✗ Profile priority conflicts failed"
  echo "Expected CONFLICT_VAR=high-priority, got:"
  echo "$output" | grep CONFLICT_VAR
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
output=$(devenv --profile function-profile --profile attrset-profile print-dev-env)
if echo "$output" | grep -q 'TEST_VAR.*foobar' && echo "$output" | grep -q 'BASE_ENV.*foobar'; then
  echo "✓ Profile priorities working correctly - attrset-profile overrides function-profile, profiles override base config"
else
  echo "✗ Profile precedence failed"
  echo "Expected TEST_VAR=foobar and BASE_ENV=foobar, got:"
  echo "$output" | grep -E "(TEST_VAR|BASE_ENV)"
  exit 1
fi

echo "All profile tests passed!"
