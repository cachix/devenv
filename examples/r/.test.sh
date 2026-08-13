#!/usr/bin/env bash
set -euxo pipefail

command -v radian
R --vanilla --slave -e 'stopifnot(requireNamespace("languageserver", quietly = TRUE))'
