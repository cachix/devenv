#!/usr/bin/env bash
set -xe 
devenv build languages.python.package
devenv shell ls -la