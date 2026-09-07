#!/usr/bin/env bash
set -euo pipefail

# Deterministic PTY regression for the same reactive target Bombadil fuzzes.
# Arguments are explicit so CI can use Cargo's resolved binary paths.
if [[ $# -ne 2 ]]; then
  echo "usage: $0 DEVENV_RUN_TESTS TUI_REPLAY" >&2
  exit 2
fi

pty_driver=$1
tui_replay=$2
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
work_dir=$(mktemp -d)
transcript="$work_dir/transcript"
event_log="$work_dir/events.jsonl"
export TUI_REPLAY_EVENT_LOG=$event_log
export TUI_REPLAY_TRANSCRIPT=$transcript

cleanup() {
  status=$?
  if [[ $status -ne 0 ]]; then
    echo "TUI replay regression failed; semantic events:" >&2
    if [[ -f $event_log ]]; then
      cat "$event_log" >&2
    fi
    echo "Raw terminal transcript: $transcript" >&2
    if [[ -f $transcript ]]; then
      cat "$transcript" >&2
    fi
  fi
  rm -rf "$work_dir"
  exit "$status"
}
trap cleanup EXIT

fixture="$repo_root/devenv-tui/replays/processes.jsonl"
command="exec \"$tui_replay\" --hold --attached --reactive --event-log \"$event_log\" \"$fixture\""

# Avoid standalone Esc in this byte-level harness: the emulated terminal reports
# no keyboard-protocol enhancement, so legacy decoding keeps Esc ambiguous until
# another input byte arrives.
# The first restart is a pasted/key-repeat burst; it must remain one user intent.
# Wait for the stopped fixture row before navigating: under load, selecting a
# process while that row is still being inserted can clear or move the selection.
# Process rows are delivered independently: macOS can render the disabled row
# before the worker log, while Linux can render it afterward. The readiness gate
# below checks the complete transcript instead of imposing an event order.
"$pty_driver" pty --step-timeout 10 "$transcript" "$command" >/dev/null <<'EOF'
expect:Running processes
expect:api
expect:├
expect:worker
expect:processed deterministic job 1
run:while ! grep -Fq 'disabled ' "$TUI_REPLAY_TRANSCRIPT" || ! grep -Fq 'stopped' "$TUI_REPLAY_TRANSCRIPT"; do sleep 0.02; done
resize:72x18
expect:^D
resize:140x45
expect:hide stopped
send:/worker
expect:Search processes: /worker
send:\r
expect:hide preview
send:h
expect:focus
send:l
expect:processed deterministic job 1
expect:hide preview
send:h
expect:focus
send:/missing
expect:Search processes: /missing
expect:no matches
send:\r
expect:nav
send:/api
expect:Search processes: /api
send:\r
expect:focus
send:\x1b[C
expect:listening on http://127.0.0.1:8080
expect:hide preview
send:\x1b[D
expect:focus
send:\x05
expect:FOLLOWING
send:\x15
expect:PAUSED
send:\x04
expect:FOLLOWING
send:\x02
expect:PAUSED
send:\x06
expect:FOLLOWING
send:/t
expect:Search logs: /t
expect:1/3
send:\r
expect:n/N
send:n
expect:2/3
send:y
expect:bGlzdGVuaW5nIG9uIGh0dHA6Ly8xMjcuMC4wLjE6ODA4MA==
expect:Copied 1 line
send:G
expect:FOLLOWING
send:g
expect:PAUSED
send:G
expect:FOLLOWING
send:q
expect:Running processes
expect:focus
send:/api
expect:Search processes: /api
send:\r
expect:focus
send:\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12\x12
expect:restarting
expect:ready
run:test "$(tail -n 1 "$TUI_REPLAY_EVENT_LOG")" = '{"kind":"status","process":"api","status":"ready"}'
send:\x18
expect:stopping
run:while ! tail -n 1 "$TUI_REPLAY_EVENT_LOG" | grep -Fq '"process":"api","status":"stopped"'; do sleep 0.02; done
expect:^R
send:\x12
expect:restarting
expect:ready
send:\x03
expect:Detach
send:c
expect:^R
send:\x03
expect:Detach
send:s
EOF

# The screen assertions prove crossterm decoded the bytes and rendered every
# transition. The sidecar proves the commands reached the backend exactly once;
# ANSI history alone cannot establish that.
[[ $(grep -Fc '"kind":"command","command":"restart","process":"api"' "$event_log") -eq 2 ]]
[[ $(grep -Fc '"kind":"command","command":"stop","process":"api"' "$event_log") -eq 1 ]]
[[ $(grep -Fc '"kind":"command","command":"stop_manager","process":"*"' "$event_log") -eq 1 ]]
[[ $(grep -Fc '"status":"restarting"' "$event_log") -eq 2 ]]
[[ $(grep -Fc '"status":"ready"' "$event_log") -eq 2 ]]

transcript="$work_dir/configured-transcript"
event_log="$work_dir/configured-events.jsonl"
export TUI_REPLAY_EVENT_LOG=$event_log
export TUI_REPLAY_TRANSCRIPT=$transcript
user_config="$repo_root/devenv-tui/replays/user-config.yaml"
command="exec \"$tui_replay\" --hold --attached --reactive --user-config \"$user_config\" --event-log \"$event_log\" \"$fixture\""

"$pty_driver" pty --step-timeout 10 "$transcript" "$command" >/dev/null <<'EOF'
expect:Running processes
expect:api
expect:worker
expect:processed deterministic job 1
run:while ! grep -Fq 'disabled ' "$TUI_REPLAY_TRANSCRIPT" || ! grep -Fq 'stopped' "$TUI_REPLAY_TRANSCRIPT"; do sleep 0.02; done
expect:CONFIGURED
resize:64x18
resize:132x42
send:?api
expect:/api 1 match
send:\r
expect:^R
send:\x05
expect:following
send:/t
expect:/t 1/3
send:\r
expect:n next
send:n
expect:2/3
send:y
expect:bGlzdGVuaW5nIG9uIGh0dHA6Ly8xMjcuMC4wLjE6ODA4MA==
expect:Copied 1 line
send:q
expect:Running processes
expect:→
send:\x03
expect:Detach
send:c
expect:^R
send:\x03
expect:Detach
send:s
EOF

[[ $(grep -Fc '"kind":"command","command":"stop_manager","process":"*"' "$event_log") -eq 1 ]]
[[ $(grep -Foc $'\033[?1000h' "$transcript") -ge 1 ]]
[[ $(grep -Foc $'\033[?1006h' "$transcript") -ge 1 ]]

transcript="$work_dir/mouse-disabled-transcript"
event_log="$work_dir/mouse-disabled-events.jsonl"
export TUI_REPLAY_EVENT_LOG=$event_log
export TUI_REPLAY_TRANSCRIPT=$transcript
user_config="$repo_root/devenv-tui/replays/mouse-disabled-user-config.yaml"
command="exec \"$tui_replay\" --hold --attached --reactive --user-config \"$user_config\" --event-log \"$event_log\" \"$fixture\""

"$pty_driver" pty --step-timeout 10 "$transcript" "$command" >/dev/null <<'EOF'
expect:Running processes
expect:stopped
send:/api
expect:Search processes: /api
send:\r
expect:^E
send:\x05
expect:FOLLOWING
send:q
expect:Running processes
send:\x03
expect:Detach
send:s
EOF

[[ $(grep -Foc $'\033[?1000h' "$transcript") -eq 0 ]]
[[ $(grep -Foc $'\033[?1006h' "$transcript") -eq 0 ]]
[[ $(grep -Fc '"kind":"command","command":"stop_manager","process":"*"' "$event_log") -eq 1 ]]

transcript="$work_dir/piped-stdout-transcript"
event_log="$work_dir/piped-stdout-events.jsonl"
stdout_capture="$work_dir/stdout"
export TUI_REPLAY_EVENT_LOG=$event_log
export TUI_REPLAY_TRANSCRIPT=$transcript
command="exec \"$tui_replay\" --hold --attached --reactive --event-log \"$event_log\" \"$fixture\" >\"$stdout_capture\""

"$pty_driver" pty --step-timeout 10 "$transcript" "$command" >/dev/null <<'EOF'
expect:processed deterministic job 1
send:\x03
expect:Detach
send:s
EOF

[[ ! -s $stdout_capture ]]
[[ $(grep -Foc $'\033[6n' "$transcript") -ge 1 ]]
[[ $(grep -Foc $'\033[6;1H' "$transcript") -ge 1 ]]

echo "TUI replay PTY regression passed"
