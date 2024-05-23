#!/usr/bin/env bash
set -ex
env | grep FOO=1
env | grep BAR=1
env | grep CHAZ=4
env | grep BAZ=5
