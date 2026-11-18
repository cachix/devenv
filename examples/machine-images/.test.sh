#!/usr/bin/env bash
set -euo pipefail
# Keep the automatically tested example cheap. Full image and VM builds are
# explicit operations documented in the machines guide.
devenv machines images list | grep -q google-compute
python project/hello.py
test "$PROJECT_SHELL_READY" = 1
