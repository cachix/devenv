#!/usr/bin/env bash

set -euo pipefail

devenv --clean shell -- bash -euc '
  julia --version
  fatou --version
  fatou lsp --help
'

devenv --clean --option languages.julia.lsp.enable:bool false shell -- bash -euc '
  julia --version
  ! command -v fatou
'

devenv --clean --option languages.julia.lsp.package:pkg hello shell -- bash -euc '
  julia --version
  hello --version
  ! command -v fatou
'
