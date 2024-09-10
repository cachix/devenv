#!/usr/bin/env bash
set -exu
POETRY_VENV="$PWD/directory/.venv"
[ -d "$POETRY_VENV" ]
[ "$(command -v python)" = "$POETRY_VENV/bin/python" ]
python --version
poetry --version
python -c 'import requests'
cd directory
[ "$(poetry env info --path)" = "$POETRY_VENV" ]
poetry run python -c 'import requests'