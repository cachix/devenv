#!/usr/bin/env bash

set -ex
rm -rf test_proj
lake new test_proj
cd test_proj
lake exe test_proj