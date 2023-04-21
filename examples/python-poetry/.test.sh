#!/bin/sh
set -ex
[ "$(readlink .venv)" = "$PWD/.devenv/state/venv" ]
[ "$(poetry env info --path)" = "$PWD/.devenv/state/venv" ]
[ "$(command -v python)" = "$PWD/.devenv/state/venv/bin/python" ]
python --version
poetry --version
poetry run python -c 'import numpy'
python -c 'import numpy'
