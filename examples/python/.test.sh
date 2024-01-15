#!/bin/sh
set -ex
python --version | grep "3.11.7"
python -c "import requests;print(requests)"
