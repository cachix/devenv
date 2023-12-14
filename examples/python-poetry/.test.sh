#!/usr/bin/env bash
set -exu
POETRY_VENV="$PWD/.venv"
[ -d "$POETRY_VENV" ]
[ "$(poetry env info --path)" = "$POETRY_VENV" ]
[ "$(command -v python)" = "$POETRY_VENV/bin/python" ]
python --version
poetry --version
poetry run python -c 'import numpy'
python -c 'import numpy'
python -c 'import pjsua2'
