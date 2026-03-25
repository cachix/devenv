#!/usr/bin/env bash
set -exu

# Verify uv pip install works into the venv (not the Nix store).
# Regression test for https://github.com/cachix/devenv/issues/2663
uv pip install six
python -c 'import six; print(six.__version__)'
