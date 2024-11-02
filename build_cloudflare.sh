#!/usr/bin/env bash

set -xe
pip install -r requirements.txt
mkdocs build
cp _redirects site/
