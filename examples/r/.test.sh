#!/usr/bin/env bash
set -ex

R --version
radian --version

for package in readr stringr data.table yaml testthat; do
  Rscript -e "library($package)"
done
