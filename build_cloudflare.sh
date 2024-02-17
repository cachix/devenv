#!/usr/bin/env bash

set -xe
poetry install --with docs
poetry run -- mkdocs build
