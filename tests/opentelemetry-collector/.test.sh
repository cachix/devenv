#!/usr/bin/env bash
set -euo pipefail
. "$DEVENV_TEST_LIB"

check_collector() {
  local ports=$1 health metrics grpc http
  health=$(jq -r .health "$ports")
  metrics=$(jq -r .metrics "$ports")
  grpc=$(jq -r .grpc "$ports")
  http=$(jq -r .http "$ports")
  wait_until 60 curl -fsS "http://localhost:$health/"
  curl -fsS "http://localhost:$metrics/metrics" | grep otelcol_process >/dev/null
  curl -fsS -H 'Content-Type: application/json' \
    -d '{"resourceSpans":[{"scopeSpans":[{"spans":[{"traceId":"0123456789abcdef0123456789abcdef","spanId":"0123456789abcdef","name":"collector-test","startTimeUnixNano":"1700000000000000000","endTimeUnixNano":"1700000000001000000"}]}]}]}' \
    "http://localhost:$http/v1/traces"
  grpcurl -plaintext -import-path . -proto trace.proto -d '{}' \
    "localhost:$grpc" opentelemetry.proto.collector.trace.v1.TraceService/Export
  wait_until 15 has_accepted_span "$metrics"
}

has_accepted_span() {
  curl -fsS "http://localhost:$1/metrics" | grep -E '^otelcol_receiver_accepted_spans.* [1-9]' >/dev/null
}

validate-collector
check_collector "$OTEL_PORTS_FILE"

cleanup() {
  if [ -d repo2 ]; then
    (cd repo2 && devenv processes down) || true
  fi
}
trap cleanup EXIT
mkdir repo2
cp collector.nix repo2/devenv.nix
(cd repo2 && devenv --no-tui up -d) || { cat repo2/.devenv/state/otel-ports.json 2>/dev/null; exit 1; }
wait_until 60 test -s repo2/.devenv/state/otel-ports.json
second_ports=repo2/.devenv/state/otel-ports.json
check_collector "$second_ports"

# Compare every listener, not only matching port names (4317 may shift to 4318).
jq -e -s '([.[0][]] - [.[1][]]) | length == 4' "$OTEL_PORTS_FILE" "$second_ports"
echo 'Two collector projects run concurrently without listener collisions.'
