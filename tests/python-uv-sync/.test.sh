#!/usr/bin/env bash
set -exu
python --version
uv --version
python -c 'import requests'

# Test the uv2nix import functionality
echo "Testing uv2nix import..."

# Build the myapp package using uv2nix and capture the output path
myapp_path=$(devenv build outputs.myapp | jq -r '."outputs.myapp"')

# Test importing the package using the built virtualenv's python
"$myapp_path/bin/python" -c 'from python_uv_sync import hello; print(hello())'

echo "uv2nix import test completed successfully!"
