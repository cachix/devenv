#!/usr/bin/env bash
set -exu
POETRY_VENV="$PWD/.venv"
[ -d "$POETRY_VENV" ]
[ "$(poetry env info --path)" = "$POETRY_VENV" ]
[ "$(command -v python)" = "$POETRY_VENV/bin/python" ]
python --version
poetry --version
poetry run python -c "import os; print(os.environ['LD_LIBRARY_PATH'])"
ls .devenv/profile/lib
poetry run python -c 'import numpy'
python -c 'import numpy'
python -c 'import pjsua2'
