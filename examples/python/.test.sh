#!/bin/sh
set -ex
python --version | grep "3.11.3"
python -c "import requests;print(requests)"
python -c "import arrow;print(arrow.__version__)" | grep "1.2.3"
