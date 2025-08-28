#!/usr/bin/env bash
set -exu
python --version
uv --version
python -c 'import requests'

# Test the uv2nix import functionality
echo "Testing uv2nix import..."

# The myapp package should be available as a derivation
devenv build outputs.myapp

echo "uv2nix import test completed successfully!"
