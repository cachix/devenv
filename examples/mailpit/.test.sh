#!/usr/bin/env bash
set -ex

wait_for_port 1025

sendmail john@example.com <<EOF
Subject: Hello

Hello world!
EOF
