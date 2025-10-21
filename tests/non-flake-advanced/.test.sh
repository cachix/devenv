#!/usr/bin/env bash

set -x

nix-shell --run "npins init &&                                            \
                 npins add github edolstra flake-compat &&                \
                 npins add github oxalica rust-overlay -b master" -p npins
nix-shell
