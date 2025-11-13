#!/usr/bin/env bash
set -exu

echo ""
echo "================================"
echo "Testing pip profile..."
echo "================================"
devenv shell --profile pip-with-packages ./check-python-env.py

echo ""
echo "================================"
echo "Testing uv profile..."
echo "================================"
devenv shell --profile uv-with-packages ./check-python-env.py

echo ""
echo "✓ All tests passed!"
