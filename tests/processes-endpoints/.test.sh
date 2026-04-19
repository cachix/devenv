#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  devenv processes down >/dev/null 2>&1 || true
}
trap cleanup EXIT

devenv up -d
devenv processes wait

output=$(devenv processes endpoints)
printf '%s\n' "$output"

if ! grep -Fq 'app-1' <<<"$output"; then
  echo "✗ Missing app-1 in endpoints output"
  exit 1
fi

if ! grep -Fq 'app-2' <<<"$output"; then
  echo "✗ Missing app-2 in endpoints output"
  exit 1
fi

mapfile -t urls < <(grep -Eo 'http://127\.0\.0\.1:[0-9]+/' <<<"$output" | awk '!seen[$0]++')
if [ "${#urls[@]}" -ne 2 ]; then
  echo "✗ Expected exactly 2 distinct URLs, got ${#urls[@]}"
  exit 1
fi

if [ "${urls[0]}" = "${urls[1]}" ]; then
  echo "✗ Expected distinct dynamically allocated URLs"
  exit 1
fi

for url in "${urls[@]}"; do
  status=$(curl -s -o /dev/null -w '%{http_code}' "$url")
  if [ "$status" != "200" ]; then
    echo "✗ Expected HTTP 200 from $url, got $status"
    exit 1
  fi
done

echo "✓ devenv processes endpoints shows resolved dynamic URLs"
