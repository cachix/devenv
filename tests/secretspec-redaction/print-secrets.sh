#!/usr/bin/env bash
set -eu

printf 'TOKEN=%s\n' "$TEST_TOKEN"
printf 'FILE='
cat "$TEST_FILE"
printf '\n'
# Separate writes exercise the streaming state across PTY reads.
printf 'SPLIT='
printf '%s' "$TEST_TOKEN" | dd bs=1 count=8 2>/dev/null
sleep 0.1
printf '%s\n' "$TEST_TOKEN" | dd bs=1 skip=8 2>/dev/null
# These controls must survive even when numeric values are secret patterns.
printf '\033[?2004h\033[0mMODE=2004\n'
printf '\033]0;%s\007' "$TEST_TOKEN"
# Interrupted OSC/DCS controls must resume masking ordinary terminal text.
printf '\033]10;?\033[0mOSC=%s\n' "$TEST_TOKEN"
printf '\033P$qm\033[0mDCS=%s\n' "$TEST_TOKEN"
printf 'REDACTION_DONE\n'
