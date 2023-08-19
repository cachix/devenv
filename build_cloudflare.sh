#!/usr/bin/env bash
set -xe
poetry install --with docs
poetry remove mkdocs-material
poetry add "git+https://${GH_TOKEN}@github.com/squidfunk/mkdocs-material-insiders.git@9.1.18-insiders-4.37.0"
poetry run mkdocs build --config-file mkdocs.insiders.yml
