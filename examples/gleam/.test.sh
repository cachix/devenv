#!/usr/bin/env bash

set -ex
rm -rf test_proj
gleam --version
gleam new test_proj
cd test_proj
gleam test