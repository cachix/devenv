#!/usr/bin/env bash

set -xe

env | grep BAR=1
env | grep ENV=1
env | grep LOCAL=1