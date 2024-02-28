python -c "from PIL import Image"
python -c "import grpc_tools.protoc"
python -c "import transformers"

# TODO: invoke a subprocess with an old glibc and assert it doesn't crash