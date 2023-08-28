#!/bin/sh
set -ex
[ "$(command -v python)" = "$PWD/.devenv/state/venv/bin/python" ]
[ "$VIRTUAL_ENV" = "$PWD/.devenv/state/venv" ]
python --version

# urllib3 is a propagated dependency of requests
python -c "import urllib3;print(urllib3)"

python -c "import requests;print(requests)"
