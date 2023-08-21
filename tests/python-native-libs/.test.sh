#!/usr/bin/env bash
set -ex

python -c "from PIL import Image"
python -c "import grpc_tools.protoc"
python -c "import transformers"
python -c "import torch"

# TODO: invoke a subprocess with an old glibc and assert it doesn't crash