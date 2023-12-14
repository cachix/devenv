#!/usr/bin/env bash

set -xe
poetry install
poetry run -- mkdocs build
