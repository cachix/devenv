set -e

# Each profile forces a distinct capability declaration. Evaluating its shell
# checks the generated task envelope without entering the shell.
devenv --profile absent print-dev-env --json >/dev/null
devenv --profile non-overlap print-dev-env --json >/dev/null
devenv --profile overlap print-dev-env --json >/dev/null
devenv --profile absolute-path print-dev-env --json >/dev/null
devenv --profile disabled print-dev-env --json >/dev/null
devenv --profile command-override print-dev-env --json >/dev/null
devenv --profile override tasks run devenv:files
test -f override-ran
rm override-ran

invalid_log=$(mktemp)
trap 'rm -f "$invalid_log"' EXIT
for profile in outside-path parent-path; do
  if devenv --profile "$profile" print-dev-env --json >"$invalid_log" 2>&1; then
    echo "expected $profile to reject an invalid managed file path" >&2
    exit 1
  fi
  grep -qF '`files` paths must stay within `devenv.root` without parent traversal.' "$invalid_log"
done

if devenv --profile duplicate-path print-dev-env --json >"$invalid_log" 2>&1; then
  echo "expected normalized duplicate destinations to be rejected" >&2
  exit 1
fi
grep -qF 'Multiple `files` paths normalize to the same destination.' "$invalid_log"

# The fallback used by older CLIs must not follow parent symlinks outside root.
outside=$(mktemp -d)
trap 'rm -f "$invalid_log" escape; rm -rf "$outside"' EXIT
echo untouched > "$outside/sentinel"
ln -s "$outside" escape
if devenv --profile symlink-parent tasks run devenv:files >"$invalid_log" 2>&1; then
  echo "expected the shell fallback to reject a symlinked parent" >&2
  exit 1
fi
grep -qF 'Unsafe managed file parent:' "$invalid_log"
test "$(cat "$outside/sentinel")" = untouched
