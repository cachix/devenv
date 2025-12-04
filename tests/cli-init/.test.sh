#!/usr/bin/env bash
set -xe
set -o pipefail

devenv init project
cd project
devenv version
devenv test
