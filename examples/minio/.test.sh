#!/usr/bin/env bash
set -ex

wait_for_port 9000
mc admin info local