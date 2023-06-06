#!/bin/sh
set -ex
python --version | grep "3.7.16"
python -c "import requests;print(requests)"