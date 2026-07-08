#!/usr/bin/env bash

set -ex

conan profile show \
  | grep -F "build_type=Debug"

conan profile show \
  | grep -F "compiler.cppstd=14"

ls $CONAN_FLAKE_HOME/conan.lock
ls $CONAN_FLAKE_CONFIG/conan.lock.checksum

build-wrapper
find build -iname "compressor*" -type f -executable -exec "{}" ";" \
  | grep -Pzo "Uncompressed size is: 207(.|\n)Compressed size is: 149(.|\n)ZLIB VERSION: 1.3.1\n"
