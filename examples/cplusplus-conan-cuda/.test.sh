#!/usr/bin/env bash

set -ex

conan install . --build=missing
conan build . --build=missing
find build -iname "example*" -type f -executable -exec "{}" ";" \
  | grep -F "example/0.0.1"

conan create . --build=missing 2>&1 \
  | grep -F "CUBLAS Matrix Multiply is close enough to CPU results: Yes"
