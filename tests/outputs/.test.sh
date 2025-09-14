#!/usr/bin/env bash

set -x

devenv build | grep -E '(myapp1|git|myapp2|hello)'

devenv build myapp2.package | grep myapp2
