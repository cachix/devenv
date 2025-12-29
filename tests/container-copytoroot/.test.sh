set -xe

# Add required inputs for container support
devenv inputs add mk-shell-bin github:rrbutani/nix-mk-shell-bin
devenv inputs add nix2container github:nlewo/nix2container --follows nixpkgs

# Generate the test files
devenv shell true

# Test 1: Build and verify container with directory copyToRoot
echo "Testing directory copyToRoot..."
devenv container build test-dir
output=$(devenv container run test-dir)
echo "$output" | grep "test-dir"
echo "$output" | grep "file1.txt"
echo "$output" | grep "file2.txt"

# Test 2: Build and verify container with single file copyToRoot
echo "Testing single file copyToRoot..."
devenv container build test-file
output=$(devenv container run test-file)
echo "$output" | grep "test-file.txt"

# Test 3: Build and verify container with multiple paths copyToRoot
echo "Testing multiple paths copyToRoot..."
devenv container build test-multiple
output=$(devenv container run test-multiple)
echo "$output" | grep "test-file.txt"
echo "$output" | grep "test-dir"

echo "✓ All container copyToRoot tests passed"
