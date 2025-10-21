#!/usr/bin/env bash

set -x

nix-shell -p npins --run "npins init" 
nix-shell
