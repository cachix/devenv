#!/usr/bin/env bash
set -euo pipefail
# Retention option assertions run during devenv evaluation. Runtime capture and
# rotation are exercised in devenv-processes and devenv-tasks Rust tests.
echo 'Process log retention configuration assertions passed.'
