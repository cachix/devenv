#!/usr/bin/env bash
# Regression test for https://github.com/cachix/devenv/issues/2820.
# When `devenv.yaml` is missing an input that `devenv.nix` references (here
# `git-hooks`), the eval error carrying the actionable "devenv inputs add ..."
# suggestion must be printed — not shadowed by a stale Nix warning such as
# `Ignoring the client-specified setting 'system'`. The error keeps Nix's
# natural order: trace frames first, the actionable error last, so the
# suggestion lands at the bottom of the terminal output.

set -uo pipefail

output=$(devenv shell -- true 2>&1)
status=$?

if [ "$status" -eq 0 ]; then
    echo "Test failed: devenv shell should have failed but exited 0"
    echo "Output: $output"
    exit 1
fi

# Strip ANSI escapes so position checks aren't fooled by colors.
plain=$(echo "$output" | sed 's/\x1b\[[0-9;]*[A-Za-z]//g')

# The eval error must be surfaced: the actionable suggestion has to be there.
if ! echo "$plain" | grep -q "devenv inputs add git-hooks"; then
    echo "Test failed: 'devenv inputs add git-hooks' suggestion was not in the output"
    echo "Output: $output"
    exit 1
fi

# The suggestion must come after the trace frames, at the bottom of the
# output, where the user's cursor lands — not above ~100 lines of
# `--show-trace` frames that would force scrolling up to find it.
suggestion_line=$(echo "$plain" | grep -n "devenv inputs add git-hooks" | tail -n1 | cut -d: -f1)
last_frame_line=$(echo "$plain" | grep -n "… while" | tail -n1 | cut -d: -f1)
if [ -n "$last_frame_line" ] && [ "$suggestion_line" -lt "$last_frame_line" ]; then
    echo "Test failed: the suggestion (line $suggestion_line) appears above trace frames (last at line $last_frame_line)"
    echo "Output: $output"
    exit 1
fi

echo "OK: missing-input suggestion is printed below the trace"
