#!/bin/sh
set -ex

# Ensure that we are using GCC
$CC --version | grep GCC || exit "Compiler is not GCC: $($CC --version)"

# Build
src=$(dirname $(realpath $0))
tmpdir=$(mktemp -d)
cd $tmpdir
meson setup $src
ninja -C $tmpdir

# Run
./c-sdl2 --exit
