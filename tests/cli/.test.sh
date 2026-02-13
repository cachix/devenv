set -xe

rm devenv.yaml || true
devenv shell -- env | grep "DEVENV_CMDLINE"
devenv build languages.python.package
devenv shell ls -- -la | grep ".test.sh"
devenv shell ls ../ | grep "cli"
devenv info | grep "python3-"
devenv show | grep "python3-"
devenv search ncdu 2>&1 | grep -E "Found [0-9]+ packages and [0-9]+ options for 'ncdu'"
devenv --trace-output file:search-trace.log search '^ncdu$' 2>&1 | grep -E "Found [0-9]+ packages and [0-9]+ options"
grep "Cache hit" search-trace.log
devenv search xyznonexistentpackagexyz 2>&1 | grep -E "Found 0 packages and 0 options"

# there should be no processes
devenv up && exit 1

# Skip container tests on macOS
if [[ "$(uname)" == "Darwin" ]]; then
	echo "Skipping container tests on macOS"
	exit 0
fi

# containers
devenv container build shell && exit 1
devenv inputs add mk-shell-bin github:rrbutani/nix-mk-shell-bin --follows nixpkgs
devenv inputs add nix2container github:nlewo/nix2container --follows nixpkgs
devenv container build shell | grep image-shell.json
devenv gc

# Test profile error handling with no profiles defined
echo "Testing profile error handling..."
error_output=$(devenv --profile some-profile info 2>&1 || true)
if echo "$error_output" | grep -q "Profile 'some-profile' not found"; then
	echo "✓ Profile error handling works correctly"
else
	echo "✗ Profile error handling failed: $error_output"
	exit 1
fi

# Test --from flag with absolute path
echo "Testing --from with absolute path..."
from_test_dir="$(cd from-test && pwd)"
output=$(devenv --from "$from_test_dir" info)
echo "$output" | grep -q "languages.rust" || exit 1
! echo "$output" | grep -q "python3" || exit 1
echo "✓ --from with absolute path works and doesn't load local devenv.nix"

# Test --from flag with relative path
echo "Testing --from with relative path..."
output=$(devenv --from ./from-test info)
echo "$output" | grep -q "languages.rust" || exit 1
! echo "$output" | grep -q "python3" || exit 1
echo "✓ --from with relative path works and doesn't load local devenv.nix"

# Test that --from allows building without local devenv.nix
echo "Testing --from without local devenv.nix..."
mkdir -p test-from-only
cd test-from-only
output=$(devenv --from "$from_test_dir" info)
echo "$output" | grep -q "languages.rust" || exit 1
cd ..
rm -rf test-from-only
echo "✓ --from works without local devenv.nix"

# Test -f short form
echo "Testing -f short form..."
output=$(devenv -f ./from-test info)
echo "$output" | grep -q "languages.rust" || exit 1
! echo "$output" | grep -q "python3" || exit 1
echo "✓ -f short form works"
