set -xe

# Skip container tests on macOS
if [[ "$(uname)" == "Darwin" ]]; then
  echo "Skipping container tests on macOS"
  exit 0
fi

# Add required inputs for container support
devenv inputs add mk-shell-bin github:rrbutani/nix-mk-shell-bin --follows nixpkgs
devenv inputs add nix2container github:nlewo/nix2container --follows nixpkgs

# Generate the test files
devenv shell

# Test 1: Build container with directory copyToRoot
echo "Testing directory copyToRoot..."
devenv container build test-dir | grep "image-test-dir.json"

# Test 2: Build container with single file copyToRoot
echo "Testing single file copyToRoot..."
devenv container build test-file | grep "image-test-file.json"

# Test 3: Build container with multiple paths copyToRoot
echo "Testing multiple paths copyToRoot..."
devenv container build test-multiple | grep "image-test-multiple.json"

echo "✓ All container copyToRoot tests passed"
