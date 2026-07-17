#!/usr/bin/env bash
set -euxo pipefail

starship --version

# STARSHIP_CONFIG should point at the TOML generated from config.settings.
test -n "${STARSHIP_CONFIG:-}"
grep -q "success_symbol" "$STARSHIP_CONFIG"
