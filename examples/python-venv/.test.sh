#!/usr/bin/env bash
set -ex
[ "$(command -v python)" = "$DEVENV_STATE/venv/bin/python" ]
[ "$VIRTUAL_ENV" = "$DEVENV_STATE/venv" ]
python --version