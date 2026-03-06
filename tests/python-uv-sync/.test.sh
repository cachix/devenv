#!/usr/bin/env bash
set -exu
python --version
uv --version
python -c 'import requests'

# Verify venv python is first on PATH, not the Nix store python
python_path=$(which python)
if [[ "$python_path" != *"/state/venv/"* ]]; then
  echo "ERROR: python resolves to $python_path, expected venv python"
  exit 1
fi

# Test the uv2nix import functionality
echo "Testing uv2nix import..."

# Build the myapp package using uv2nix and capture the output path
myapp_path=$(devenv build outputs.myapp | jq -r '."outputs.myapp"')

# Test importing the package using the built virtualenv's python
"$myapp_path/bin/python" -c 'from python_uv_sync import hello; print(hello())'

echo "uv2nix import test completed successfully!"
