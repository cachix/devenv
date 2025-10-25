#!/usr/bin/env bash

set -ex
treefmt --version
treefmt
! diff sample.original sample.json
