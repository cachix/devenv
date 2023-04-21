#!/bin/sh
set -ex
[ "$(command -v python)" = "$PWD/.devenv/state/venv/bin/python" ]
[ "$VIRTUAL_ENV" = "$PWD/.devenv/state/venv" ]
python --version
