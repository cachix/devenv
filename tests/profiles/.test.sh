set -e

# Test profile error handling with no profiles defined
echo "Testing profile error handling..."
error_output=$(devenv --profile some-profile info 2>&1 || true)
if echo "$error_output" | grep -q "Profile 'some-profile' not found"; then
    echo "✓ Profile error handling works correctly"
else
    echo "✗ Profile error handling failed: $error_output"
    exit 1
fi

# Test 1: No profiles (base configuration)
echo "Test 1: Base configuration (no profiles)"
if devenv print-dev-env | grep -q 'BASE_ENV.*base-value'; then
    echo "✓ Base environment variable found"
else
    echo "✗ Base environment variable not found"
    exit 1
fi

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

echo "All profile tests passed!"