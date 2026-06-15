#!/usr/bin/env bash

set -ex

conan profile show \
  | grep -F "build_type=Debug"

conan profile show \
  | grep -F "compiler.cppstd=14"
