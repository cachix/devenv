#!/usr/bin/env bash

# Verify the `start.enable = "shell"` schema:
#  - "shell" processes still start with `devenv up` (decision: up starts them too)
#  - `false` processes do not start with `devenv up`
#  - `process.shellStartProcesses` lists only the "shell" processes

set -ex

PORT_UP=18551
PORT_SHELL=18552
PORT_OFF=18553

reachable() {
  curl -s -o /dev/null --connect-timeout 1 "http://127.0.0.1:$1/" 2>/dev/null
}

wait_for_port() {
  for _ in $(seq 1 30); do
    if reachable "$1"; then return 0; fi
    sleep 1
  done
  return 1
}

# --- shellStartProcesses lists only the "shell" process ---
shell_list=$(devenv eval process.shellStartProcesses)
echo "shellStartProcesses: $shell_list"
echo "$shell_list" | grep -q "shell_proc" || { echo "FAIL: shell_proc missing from shellStartProcesses"; exit 1; }
echo "$shell_list" | grep -q "up_proc" && { echo "FAIL: up_proc should not be in shellStartProcesses"; exit 1; }
echo "$shell_list" | grep -q "off_proc" && { echo "FAIL: off_proc should not be in shellStartProcesses"; exit 1; }

# --- devenv up starts both `true` and `"shell"` processes, but not `false` ---
devenv up -d
devenv processes wait

wait_for_port "$PORT_UP" || { echo "FAIL: up_proc (start.enable=true) did not start"; devenv processes down || true; exit 1; }
wait_for_port "$PORT_SHELL" || { echo "FAIL: shell_proc (start.enable=\"shell\") did not start under up"; devenv processes down || true; exit 1; }

if reachable "$PORT_OFF"; then
  echo "FAIL: off_proc (start.enable=false) should not have started"
  devenv processes down || true
  exit 1
fi

devenv processes down

echo "All process-start-shell tests passed!"
