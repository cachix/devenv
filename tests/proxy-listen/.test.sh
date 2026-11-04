#!/usr/bin/env bash
set -euo pipefail

# Acquire the fixture's tools, but run every CLI command in a fresh process.
devenv shell -- python3 .test.py
