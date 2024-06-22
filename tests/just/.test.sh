#!/usr/bin/env bash
set -ex
just | grep "Generate CHANGELOG.md using recent commits"
just | grep "test hello"
just | grep "Hello Script"
