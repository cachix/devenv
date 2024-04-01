#!/usr/bin/env bash

set -ex
gleam --version
gleam new test_proj
cd test_proj
gleam test
