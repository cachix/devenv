#!/usr/bin/env bash
set -ex

wait_for_port $MINIO_PORT
mc admin info local