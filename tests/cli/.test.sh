#!/usr/bin/env bash
set -euo pipefail

#
# Smoke tests for the devenv CLI surface.
#

step() { echo; echo "── $* ──"; }
fail() { echo "✗ $*" >&2; exit 1; }

rm -f devenv.yaml

step "shell exports cmdline + task-defined env vars"
devenv shell -- env | grep -q DEVENV_CMDLINE
devenv shell -- env | grep -q "DEVENV_CLI_TEST_VAR=hello-from-task"

step "build + inspect a single package attribute"
devenv build languages.python.package

step "shell forwards arguments verbatim and to subdirs"
devenv shell ls -- -la | grep -q "\.test\.sh"
devenv shell ls ../ | grep -q "cli"

step "shell accepts command strings and literal arguments with or without --"
for separator in '' --; do
  shell_args=(shell)
  if [[ -n "$separator" ]]; then shell_args+=("$separator"); fi
  output=$(devenv "${shell_args[@]}" "printf '<%s>\\n' 'two words'")
  [[ "$output" == '<two words>' ]] \
    || fail "quoted shell command failed with separator '$separator'"
  output=$(devenv "${shell_args[@]}" "printf '<%s>\\n'" 'two words' '$HOME')
  [[ "$output" == $'<two words>\n<$HOME>' ]] \
    || fail "quoted shell command changed literal arguments with separator '$separator'"
  output=$(devenv "${shell_args[@]}" printf '<%s>\n' 'two words' '$HOME')
  [[ "$output" == $'<two words>\n<$HOME>' ]] \
    || fail "shell changed literal arguments with separator '$separator'"
  output=$(devenv "${shell_args[@]}" 'printf "<%s>\n" $# $1 "$1"' foo)
  [[ "$output" == $'<0>\n<>\n<foo>' ]] \
    || fail "quoted shell command saw positional parameters with separator '$separator'"
done

step "shell keeps commands and arguments out of the activation script"
devenv shell -- 'printf "%s" "devenv-cli-command-marker"' 'devenv-cli-argument-marker' >/dev/null
rc=0
grep -F -e 'devenv-cli-command-marker' -e 'devenv-cli-argument-marker' .devenv/shell-*.sh >/dev/null || rc=$?
[[ "$rc" == 1 ]] || fail "activation scripts contain command text or could not be read (grep exited $rc)"

step "shell passes unusual arguments literally"
output=$(devenv shell -- printf '<%s>\n' '' '$(echo no)' '`echo no`' '*' "it's" 'a\b' 'a"b' $'a\nb')
[[ "$output" == $'<>\n<$(echo no)>\n<`echo no`>\n<*>\n<it\'s>\n<a\\b>\n<a"b>\n<a\nb>' ]] \
  || fail "shell changed unusual arguments"

step "shell command strings see the activated environment"
output=$(devenv shell -- 'printf "<%s>\n" "$DEVENV_CLI_TEST_VAR" "$command"')
[[ "$output" == $'<hello-from-task>\n<env-command>' ]] \
  || fail "quoted shell command did not see the activated environment: $output"

step "shell forwards flags after -- and preserves the exit status"
output=$(devenv shell -- printf '%s\n' --help)
[[ "$output" == '--help' ]] || fail "shell consumed --help after --: $output"
output=$(devenv shell bash -- -c 'echo ok')
[[ "$output" == 'ok' ]] || fail "shell broke -- after the command: $output"
rc=0
devenv shell -- sh -c 'exit 3' || rc=$?
[[ "$rc" == 3 ]] || fail "shell did not preserve exit status 3, got $rc"

step "info/show surface enabled languages"
devenv info | grep -q "python3-"
devenv show | grep -q "python3-"

step "search returns packages and writes trace cache hits"
devenv search ncdu 2>&1 \
  | grep -Eq "Found [0-9]+ packages and [0-9]+ options for 'ncdu'"
RUST_LOG=trace devenv --verbose --trace-output file:search-trace.log \
  search '^ncdu$' 2>&1 \
  | grep -Eq "Found [0-9]+ packages and [0-9]+ options"
grep "cache hit" search-trace.log | grep -q optionsJSON \
  || fail "expected an optionsJSON cache hit in search-trace.log"
devenv search xyznonexistentpackagexyz 2>&1 \
  | grep -Eq "Found 0 packages and 0 options"

step "up fails when no processes are defined"
if devenv up; then fail "devenv up should fail without processes"; fi

step "unknown profile is reported clearly"
out=$(devenv --profile some-profile info 2>&1 || true)
echo "$out" | grep -q "Profile 'some-profile' not found" \
  || fail "expected 'Profile not found' error, got: $out"

step "--from loads an external project and ignores local devenv.nix"
from_test_dir="$(cd from-test && pwd)"
# from-test enables languages.rust, this project enables languages.python.
# Pulling rust-toolchain into the package list while dropping python3 proves
# we used the --from devenv.nix, not the local one.
for path in "path:$from_test_dir" "path:./from-test"; do
  out=$(devenv --from "$path" info)
  echo "$out" | grep -q "rust-toolchain" || fail "--from=$path missing rust toolchain"
  ! echo "$out" | grep -q "python3" || fail "--from=$path leaked local python3"
done

step "--from ignores the local devenv.local.nix"
mkdir -p test-local-override && pushd test-local-override >/dev/null
cat > devenv.nix <<'EOF'
{ ... }:
{
  processes.local-only.exec = "sleep 100";
}
EOF
# A local devenv.local.nix must not be merged into an external --from project.
# It references a process defined only by the local devenv.nix, so if it leaks
# into the external project the eval fails on an undefined `exec`.
cat > devenv.local.nix <<'EOF'
{ ... }:
{
  processes.local-only.process-compose.disabled = true;
}
EOF
out=$(devenv --from "path:$from_test_dir" info) \
  || fail "--from failed with a local devenv.local.nix present"
echo "$out" | grep -q "rust-toolchain" \
  || fail "--from=$from_test_dir missing rust toolchain"
popd >/dev/null
rm -rf test-local-override

step "--from works in a directory without devenv.nix"
mkdir -p test-from-only && pushd test-from-only >/dev/null
out=$(devenv --from "path:$from_test_dir" info)
echo "$out" | grep -q "rust-toolchain" || fail "--from didn't load remote project"
popd >/dev/null
rm -rf test-from-only

step "-O packages:pkgs appends ad-hoc packages"
devenv -O packages:pkgs "hello" shell -- hello | grep -q "Hello, world"

# Containers are Linux-only.
if [[ "$(uname)" == "Linux" ]]; then
  step "container build fails without required inputs, then succeeds"
  if devenv container build shell; then fail "container build should fail without inputs"; fi
  devenv inputs add mk-shell-bin github:rrbutani/nix-mk-shell-bin --follows nixpkgs
  devenv inputs add nix2container github:nlewo/nix2container --follows nixpkgs
  devenv container build shell | grep -q image-shell.json
  devenv gc
fi
